//! A wallet on one pool: its state, and the operations it can build.

use alloy::primitives::{Address, Bytes};
use rand_core::CryptoRng;

use crate::{
    address::{Account, TacitAddress},
    field::Fr,
    keys::KeyError,
    memo::{random_secp, seal},
    pool::Pool,
    state::{OwnedNote, SavedState, StateError, WalletState},
    statement::{ExtData, OutputNote, SpendInput, StatementError, asset_fr, build},
    transaction::{Kind, Transaction},
    tree::IncTree,
};

/// Who sends a spend and what it pays them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Payer {
    /// The relayer the proof names, or the zero address when the user's own account sends it.
    pub relayer: Address,
    /// The relayer's fee in wei, paid from the shielded funds.
    pub fee: u128,
}

impl Payer {
    /// The user's own account sends the transaction and pays its gas; no fee.
    pub const SELF: Self = Self {
        relayer: Address::ZERO,
        fee: 0,
    };
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SpendError {
    #[error("insufficient balance: {required} wei needed, {available} available")]
    InsufficientBalance { required: u128, available: u128 },
    #[error(
        "the balance ({available}) covers {required} only across more than two notes; consolidate first"
    )]
    Fragmented { required: u128, available: u128 },
    #[error("there are not two notes to merge")]
    NothingToConsolidate,
    #[error("the amount must be positive")]
    ZeroAmount,
    #[error("the recipient must be a non-zero address")]
    ZeroRecipient,
    #[error(transparent)]
    Statement(#[from] StatementError),
    #[error(transparent)]
    Key(#[from] KeyError),
}

/// A wallet on one pool.
///
/// Holds the confirmed state (what [`Wallet::save`] returns) and a view that adds the newest,
/// unconfirmed blocks. Balances, notes and new transactions use the view.
#[derive(Debug, Clone)]
pub struct Wallet {
    pub(crate) account: Account,
    pub(crate) pool: Pool,
    pub(crate) asset: Fr,
    pub(crate) saved: WalletState,
    pub(crate) view: WalletState,
}

impl Wallet {
    /// A wallet that has read nothing yet.
    #[must_use]
    pub fn new(account: Account, pool: Pool) -> Self {
        let state = WalletState::new(pool.deploy_block);
        let asset = asset_fr(pool.chain_id, pool.address, Address::ZERO);
        Self {
            account,
            pool,
            asset,
            saved: state.clone(),
            view: state,
        }
    }

    /// A wallet resuming from a saved state.
    ///
    /// # Errors
    /// Returns an error if the saved state is malformed.
    pub fn restore(account: Account, pool: Pool, saved: SavedState) -> Result<Self, StateError> {
        let state = WalletState::from_saved(saved, &account)?;
        let asset = asset_fr(pool.chain_id, pool.address, Address::ZERO);
        Ok(Self {
            account,
            pool,
            asset,
            saved: state.clone(),
            view: state,
        })
    }

    /// The confirmed state, to persist and pass to [`Wallet::restore`].
    #[must_use]
    pub fn save(&self) -> SavedState {
        self.saved.to_saved()
    }

    /// Forgets everything read, so the next sync reads the pool from its first block.
    pub fn reset(&mut self) {
        self.saved = WalletState::new(self.pool.deploy_block);
        self.view = self.saved.clone();
    }

    #[must_use]
    pub fn account(&self) -> &Account {
        &self.account
    }

    #[must_use]
    pub fn address(&self) -> &TacitAddress {
        self.account.address()
    }

    #[must_use]
    pub fn pool(&self) -> &Pool {
        &self.pool
    }

    /// The wallet's view of the pool, through the newest block read.
    #[must_use]
    pub fn state(&self) -> &WalletState {
        &self.view
    }

    /// The tree to prove against when the user's own account sends the transaction.
    #[must_use]
    pub fn tree(&self) -> &IncTree {
        &self.view.tree
    }

    #[must_use]
    pub fn balance(&self) -> u128 {
        self.view.balance()
    }

    pub fn notes(&self) -> impl Iterator<Item = &OwnedNote> {
        self.view.notes()
    }

    /// Deposits `amount` wei for `to` (this wallet by default). The user's own account sends it,
    /// attaching `amount`.
    ///
    /// # Errors
    /// Returns an error if the amount is zero or too large.
    pub fn shield(
        &self,
        amount: u128,
        to: Option<&TacitAddress>,
        rng: &mut impl CryptoRng,
    ) -> Result<Transaction, SpendError> {
        if amount == 0 {
            return Err(SpendError::ZeroAmount);
        }
        let to = to.unwrap_or(self.address());
        self.build(
            Kind::Shield,
            self.tree(),
            &[],
            [Some((to, amount)), None],
            i128::try_from(amount).map_err(|_| KeyError::ValueOutOfRange)?,
            Address::ZERO,
            Payer::SELF,
            amount,
            rng,
        )
    }

    /// Pays `amount` wei to another pool address, privately. The recipient receives all of
    /// `amount`; the fee comes on top. The payment always has a second output (change, or a
    /// zero-value note), so one without change looks like one with.
    ///
    /// `tree` is the tree to prove against: [`Wallet::tree`], or a relayer's view that adds the
    /// insertions it has queued.
    ///
    /// # Errors
    /// Returns an error if the balance does not cover `amount` plus the fee in two notes.
    pub fn transfer(
        &self,
        to: &TacitAddress,
        amount: u128,
        payer: Payer,
        tree: &IncTree,
        rng: &mut impl CryptoRng,
    ) -> Result<Transaction, SpendError> {
        if amount == 0 {
            return Err(SpendError::ZeroAmount);
        }
        let need = amount
            .checked_add(payer.fee)
            .ok_or(KeyError::ValueOutOfRange)?;
        let ins = self.pick(need)?;
        let change = total(&ins) - need;
        self.build(
            Kind::Transfer,
            tree,
            &ins,
            [Some((to, amount)), Some((self.address(), change))],
            0,
            Address::ZERO,
            payer,
            amount,
            rng,
        )
    }

    /// Withdraws `amount` wei to the public address `recipient`.
    ///
    /// # Errors
    /// Returns an error if the balance does not cover `amount` plus the fee in two notes.
    pub fn unshield(
        &self,
        recipient: Address,
        amount: u128,
        payer: Payer,
        tree: &IncTree,
        rng: &mut impl CryptoRng,
    ) -> Result<Transaction, SpendError> {
        if amount == 0 {
            return Err(SpendError::ZeroAmount);
        }
        if recipient == Address::ZERO {
            return Err(SpendError::ZeroRecipient);
        }
        let need = amount
            .checked_add(payer.fee)
            .ok_or(KeyError::ValueOutOfRange)?;
        let ins = self.pick(need)?;
        let change = total(&ins) - need;
        let ext_amount = -i128::try_from(amount).map_err(|_| KeyError::ValueOutOfRange)?;
        self.build(
            Kind::Unshield,
            tree,
            &ins,
            [self.change(change), None],
            ext_amount,
            recipient,
            payer,
            amount,
            rng,
        )
    }

    /// Merges the two largest notes into one, for a balance held in more notes than one spend can
    /// take.
    ///
    /// # Errors
    /// Returns an error if there are fewer than two notes or the fee exceeds them.
    pub fn consolidate(
        &self,
        payer: Payer,
        tree: &IncTree,
        rng: &mut impl CryptoRng,
    ) -> Result<Transaction, SpendError> {
        let mut notes: Vec<&OwnedNote> = self.notes().collect();
        notes.sort_by_key(|n| std::cmp::Reverse(n.value));
        notes.truncate(2);
        if notes.len() < 2 {
            return Err(SpendError::NothingToConsolidate);
        }
        let available = total(&notes);
        let merged = available.checked_sub(payer.fee).filter(|m| *m > 0).ok_or(
            SpendError::InsufficientBalance {
                required: payer.fee,
                available,
            },
        )?;
        self.build(
            Kind::Consolidate,
            tree,
            &notes,
            [self.change(merged), None],
            0,
            Address::ZERO,
            payer,
            merged,
            rng,
        )
    }

    fn change(&self, value: u128) -> Option<(&TacitAddress, u128)> {
        (value > 0).then_some((self.address(), value))
    }

    /// Up to two unspent notes that cover `need`, or why there are none.
    fn pick(&self, need: u128) -> Result<Vec<&OwnedNote>, SpendError> {
        let notes: Vec<&OwnedNote> = self.notes().collect();
        if let Some(picked) = select_notes(&notes, need) {
            return Ok(picked);
        }
        let available = total(&notes);
        if available >= need {
            Err(SpendError::Fragmented {
                required: need,
                available,
            })
        } else {
            Err(SpendError::InsufficientBalance {
                required: need,
                available,
            })
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build(
        &self,
        kind: Kind,
        tree: &IncTree,
        ins: &[&OwnedNote],
        outs: [Option<(&TacitAddress, u128)>; 2],
        ext_amount: i128,
        recipient: Address,
        payer: Payer,
        amount: u128,
        rng: &mut impl CryptoRng,
    ) -> Result<Transaction, SpendError> {
        let sealed = [0, 1].map(|k| {
            outs[k]
                .map(|(to, value)| seal(to, value, self.asset, &random_secp(rng)))
                .transpose()
        });
        let [sealed0, sealed1] = sealed;
        let sealed = [sealed0?, sealed1?];
        let memo = |k: usize| {
            sealed[k]
                .as_ref()
                .map_or_else(Bytes::new, |s| Bytes::copy_from_slice(&s.memo))
        };
        let ext = ExtData {
            chain_id: self.pool.chain_id,
            pool: self.pool.address,
            recipient,
            ext_amount,
            relayer: payer.relayer,
            fee: payer.fee,
            memo0: memo(0),
            memo1: memo(1),
        };
        let input = |k: usize| {
            ins.get(k).map_or(SpendInput::Filler, |n| SpendInput::Note {
                value: n.value,
                rho: n.rho,
                nk: n.nk,
                sk: n.sk,
                index: n.index,
            })
        };
        let outputs = sealed.each_ref().map(|s| {
            s.as_ref().map(|s| OutputNote {
                value: s.value,
                npk: s.npk,
                rho: s.rho,
            })
        });
        let statement = build(
            self.asset,
            tree,
            [input(0), input(1)],
            outputs,
            ext_amount,
            payer.fee,
            ext.hash_fr()?,
            rng,
        )?;
        Ok(Transaction {
            kind,
            statement,
            ext,
            amount,
        })
    }
}

fn total(notes: &[&OwnedNote]) -> u128 {
    notes.iter().map(|n| n.value).sum()
}

/// Up to two notes that cover `need`: the smallest single note that does, else the first pair
/// that does, trying the largest note first. `None` when no two notes do.
fn select_notes<'a>(notes: &[&'a OwnedNote], need: u128) -> Option<Vec<&'a OwnedNote>> {
    let mut sorted = notes.to_vec();
    sorted.sort_by_key(|n| n.value);
    if let Some(one) = sorted.iter().find(|n| n.value >= need) {
        return Some(vec![*one]);
    }
    for i in (1..sorted.len()).rev() {
        for j in (0..i).rev() {
            if sorted[i].value + sorted[j].value >= need {
                return Some(vec![sorted[i], sorted[j]]);
            }
        }
    }
    None
}
