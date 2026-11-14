//! What a wallet knows of the pool: the tree through some block, and its own unspent notes in it.

use std::collections::{BTreeMap, HashSet};

use alloy::primitives::Bytes;
use ruint::aliases::U256;
use serde::{Deserialize, Serialize};

use crate::{
    address::Account,
    event::TransactEvent,
    field::{Fr, Scalar, fr, word},
    keys::{KeyError, nullifier_of, owned_keys},
    memo::open,
    tree::{IncTree, IncTreeState, TreeError},
};

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StateError {
    #[error("leaf {got} arrived out of order (expected {expected}); rescan to rebuild the history")]
    OutOfOrder { expected: u64, got: u64 },
    #[error(transparent)]
    Tree(#[from] TreeError),
    #[error(transparent)]
    Key(#[from] KeyError),
    #[error("invalid saved state")]
    InvalidSaved,
}

/// An unspent note of this wallet. `Debug` leaves out its keys and secret.
#[derive(Clone)]
pub struct OwnedNote {
    /// The note's leaf index.
    pub index: u64,
    pub leaf: U256,
    /// Its value in wei.
    pub value: u128,
    /// The block it was inserted in.
    pub block: u64,
    pub(crate) rho: Fr,
    pub(crate) secret: [u8; 33],
    pub(crate) sk: Scalar,
    pub(crate) nk: Scalar,
    pub(crate) nullifier: Fr,
}

impl std::fmt::Debug for OwnedNote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnedNote")
            .field("index", &self.index)
            .field("leaf", &self.leaf)
            .field("value", &self.value)
            .field("block", &self.block)
            .finish_non_exhaustive()
    }
}

impl OwnedNote {
    fn new(
        account: &Account,
        index: u64,
        leaf: Fr,
        value: u128,
        block: u64,
        secret: [u8; 33],
    ) -> Result<Self, KeyError> {
        let keys = owned_keys(&account.keys, &secret)?;
        Ok(Self {
            index,
            leaf: word(leaf),
            value,
            block,
            rho: keys.rho,
            secret,
            sk: keys.sk,
            nk: keys.nk,
            nullifier: nullifier_of(keys.nk, leaf, index)?,
        })
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(
        account: &Account,
        index: u64,
        leaf: Fr,
        value: u128,
        block: u64,
        secret: [u8; 33],
    ) -> Result<Self, KeyError> {
        Self::new(account, index, leaf, value, block, secret)
    }

    /// The note's nullifier: it appears on chain when the note is spent.
    #[must_use]
    pub fn nullifier(&self) -> U256 {
        word(self.nullifier)
    }
}

/// The pool through `block`, and this wallet's unspent notes in it.
#[derive(Clone, Debug)]
pub struct WalletState {
    pub(crate) block: u64,
    pub(crate) tree: IncTree,
    pub(crate) notes: BTreeMap<u64, OwnedNote>,
}

impl WalletState {
    /// Nothing read yet: the state before the pool's first block.
    #[must_use]
    pub fn new(deploy_block: u64) -> Self {
        Self {
            block: deploy_block.saturating_sub(1),
            tree: IncTree::new(),
            notes: BTreeMap::new(),
        }
    }

    /// The last block this state covers.
    #[must_use]
    pub fn block(&self) -> u64 {
        self.block
    }

    #[must_use]
    pub fn tree(&self) -> &IncTree {
        &self.tree
    }

    /// The unspent notes, by leaf index.
    pub fn notes(&self) -> impl Iterator<Item = &OwnedNote> {
        self.notes.values()
    }

    #[must_use]
    pub fn balance(&self) -> u128 {
        self.notes.values().map(|n| n.value).sum()
    }

    /// `events` (in chain order) applied to a copy of this state: their leaves go into the tree,
    /// the memos that open for `account` become tracked notes, and notes whose nullifier appears
    /// are dropped. The pool is rebuilt from the events alone, so a gap or a reordering is an
    /// error.
    pub(crate) fn absorb(
        &self,
        events: &[TransactEvent],
        account: &Account,
        asset: Fr,
    ) -> Result<Self, StateError> {
        let mut tree = self.tree.clone();
        let mut notes = self.notes.clone();
        let mut spent = HashSet::new();
        let mut batch = Vec::new();
        let mut track = Vec::new();

        for event in events {
            for nf in event.nullifiers {
                if nf != U256::ZERO {
                    spent.insert(nf);
                }
            }
            if event.out_leaves == [U256::ZERO; 2] {
                continue;
            }
            let expected = tree.size() + batch.len() as u64;
            if event.first_index != expected {
                return Err(StateError::OutOfOrder {
                    expected,
                    got: event.first_index,
                });
            }
            let leaves = [
                fr(event.out_leaves[0]).ok_or(TreeError::InvalidState)?,
                fr(event.out_leaves[1]).ok_or(TreeError::InvalidState)?,
            ];
            for (k, (&leaf, memo)) in leaves.iter().zip(&event.memos).enumerate() {
                let memo: &Bytes = memo;
                if event.out_leaves[k] == U256::ZERO || memo.is_empty() {
                    continue;
                }
                let Some(opened) = open(account, memo, leaf, asset) else {
                    continue;
                };
                if opened.value == 0 {
                    continue;
                }
                let index = event.first_index + k as u64;
                track.push(index);
                notes.insert(
                    index,
                    OwnedNote::new(
                        account,
                        index,
                        leaf,
                        opened.value,
                        event.block,
                        opened.secret,
                    )?,
                );
            }
            batch.extend(leaves);
        }
        tree.append(&batch, &track)?;

        notes.retain(|index, note| {
            let keep = !spent.contains(&word(note.nullifier));
            if !keep {
                tree.untrack(*index);
            }
            keep
        });
        Ok(Self {
            block: self.block,
            tree,
            notes,
        })
    }
}

/// A note as it is saved: what the memo revealed. The spend keys are derived again on load.
/// `Debug` leaves out the secret.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedNote {
    pub index: u64,
    pub leaf: U256,
    pub value: u128,
    pub block: u64,
    /// The note's shared secret, hex.
    pub secret: String,
}

impl std::fmt::Debug for SavedNote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SavedNote")
            .field("index", &self.index)
            .field("leaf", &self.leaf)
            .field("value", &self.value)
            .field("block", &self.block)
            .finish_non_exhaustive()
    }
}

/// The serialized form of a [`WalletState`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedState {
    pub block: u64,
    pub tree: IncTreeState,
    pub notes: Vec<SavedNote>,
}

impl WalletState {
    #[must_use]
    pub fn to_saved(&self) -> SavedState {
        SavedState {
            block: self.block,
            tree: IncTreeState::from(&self.tree),
            notes: self
                .notes
                .values()
                .map(|n| SavedNote {
                    index: n.index,
                    leaf: n.leaf,
                    value: n.value,
                    block: n.block,
                    secret: hex::encode(n.secret),
                })
                .collect(),
        }
    }

    /// Restores a saved state for `account`.
    ///
    /// # Errors
    /// Returns an error if the saved state is malformed or a note is not `account`'s.
    pub fn from_saved(saved: SavedState, account: &Account) -> Result<Self, StateError> {
        let tree = IncTree::try_from(saved.tree)?;
        let notes = saved
            .notes
            .into_iter()
            .map(|n| {
                let secret: [u8; 33] = hex::decode(&n.secret)
                    .ok()
                    .and_then(|b| b.try_into().ok())
                    .ok_or(StateError::InvalidSaved)?;
                let leaf = fr(n.leaf).ok_or(StateError::InvalidSaved)?;
                if !tree.is_tracked(n.index) {
                    return Err(StateError::InvalidSaved);
                }
                let note = OwnedNote::new(account, n.index, leaf, n.value, n.block, secret)?;
                Ok((n.index, note))
            })
            .collect::<Result<_, StateError>>()?;
        Ok(Self {
            block: saved.block,
            tree,
            notes,
        })
    }
}
