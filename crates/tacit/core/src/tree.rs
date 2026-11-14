//! The pool's Poseidon Merkle tree, kept incrementally: its right edge, its size and root, and the
//! sibling path of each tracked leaf. Memory stays small however large the pool grows, and a batch
//! of `n` leaves costs about `2n` hashes.

use std::{collections::BTreeMap, sync::OnceLock};

use ark_ff::AdditiveGroup;
use ruint::aliases::U256;
use serde::{Deserialize, Serialize};

use crate::{
    field::{Fr, fr, word},
    poseidon::poseidon,
};

/// The depth of the pool's tree.
pub const TREE_DEPTH: usize = kohaku_tacit_circuit::TREE_DEPTH;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum TreeError {
    #[error("the tree is full")]
    Full,
    #[error("tracked index {0} is outside the batch")]
    TrackOutsideBatch(u64),
    #[error("leaf {0} is not tracked")]
    NotTracked(u64),
    #[error("invalid saved tree")]
    InvalidState,
}

/// The hash of an empty subtree at each level, `0..=TREE_DEPTH`.
pub(crate) fn empty_hashes() -> &'static [Fr; TREE_DEPTH + 1] {
    static ZEROS: OnceLock<[Fr; TREE_DEPTH + 1]> = OnceLock::new();
    ZEROS.get_or_init(|| {
        let mut zeros = [Fr::ZERO; TREE_DEPTH + 1];
        for level in 1..=TREE_DEPTH {
            zeros[level] = poseidon(&[zeros[level - 1], zeros[level - 1]]);
        }
        zeros
    })
}

/// An incremental view of the pool's tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncTree {
    size: u64,
    root: Fr,
    /// The last even node at each level: the left sibling of anything appended next.
    filled: [Fr; TREE_DEPTH],
    tracked: BTreeMap<u64, [Option<Fr>; TREE_DEPTH]>,
}

impl Default for IncTree {
    fn default() -> Self {
        Self::new()
    }
}

impl IncTree {
    /// An empty tree.
    #[must_use]
    pub fn new() -> Self {
        let zeros = empty_hashes();
        Self {
            size: 0,
            root: zeros[TREE_DEPTH],
            filled: std::array::from_fn(|i| zeros[i]),
            tracked: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn size(&self) -> u64 {
        self.size
    }

    #[must_use]
    pub fn root(&self) -> U256 {
        word(self.root)
    }

    pub(crate) fn root_fr(&self) -> Fr {
        self.root
    }

    /// Appends `leaves` at the end of the tree, keeping a path for the absolute indices in
    /// `track`.
    ///
    /// # Errors
    /// Returns an error if the tree would overflow or a tracked index is outside the batch.
    pub(crate) fn append(&mut self, leaves: &[Fr], track: &[u64]) -> Result<(), TreeError> {
        if leaves.is_empty() {
            return Ok(());
        }
        let start = self.size;
        let count = leaves.len() as u64;
        if start + count > 1 << TREE_DEPTH {
            return Err(TreeError::Full);
        }
        for &index in track {
            if index < start || index >= start + count {
                return Err(TreeError::TrackOutsideBatch(index));
            }
            self.tracked.insert(index, [None; TREE_DEPTH]);
        }

        let zeros = empty_hashes();
        let mut level = leaves.to_vec();
        let mut lo = start;
        for depth in 0..TREE_DEPTH {
            let hi = lo + level.len() as u64 - 1;
            let left = self.filled[depth];
            for (&index, path) in &mut self.tracked {
                let sibling = (index >> depth) ^ 1;
                if (lo..=hi).contains(&sibling) {
                    path[depth] = Some(level[(sibling - lo) as usize]);
                } else if path[depth].is_none() {
                    path[depth] = Some(if sibling < lo { left } else { zeros[depth] });
                }
            }
            let last_even = if hi.is_multiple_of(2) { hi } else { hi - 1 };
            if last_even >= lo {
                self.filled[depth] = level[(last_even - lo) as usize];
            }
            let (next_lo, next_hi) = (lo / 2, hi / 2);
            let next: Vec<Fr> = (next_lo..=next_hi)
                .map(|pair| {
                    let l = if 2 * pair < lo {
                        left
                    } else {
                        level[(2 * pair - lo) as usize]
                    };
                    let r = if 2 * pair < hi {
                        level[(2 * pair + 1 - lo) as usize]
                    } else {
                        zeros[depth]
                    };
                    poseidon(&[l, r])
                })
                .collect();
            level = next;
            lo = next_lo;
        }
        self.root = level[0];
        self.size = start + count;
        Ok(())
    }

    /// Appends `leaves` (field elements) at the end of the tree, such as the insertions a relayer
    /// has queued after the pool's head. Paths of tracked leaves are kept current.
    ///
    /// # Errors
    /// Returns an error if a leaf is not a field element or the tree would overflow.
    pub fn append_leaves(&mut self, leaves: &[U256]) -> Result<(), TreeError> {
        let leaves = leaves
            .iter()
            .map(|x| fr(*x).ok_or(TreeError::InvalidState))
            .collect::<Result<Vec<_>, _>>()?;
        self.append(&leaves, &[])
    }

    #[must_use]
    pub fn is_tracked(&self, index: u64) -> bool {
        self.tracked.contains_key(&index)
    }

    /// The sibling path of a tracked leaf, leaf level first.
    pub(crate) fn path(&self, index: u64) -> Result<[Fr; TREE_DEPTH], TreeError> {
        let path = self
            .tracked
            .get(&index)
            .ok_or(TreeError::NotTracked(index))?;
        Ok(path.map(|x| x.expect("filled once appended")))
    }

    pub(crate) fn untrack(&mut self, index: u64) {
        self.tracked.remove(&index);
    }

    /// Siblings from level 1 to the root of the next pair slot: the path an insertion proves
    /// against.
    pub(crate) fn insertion_path(&self) -> [Fr; TREE_DEPTH - 1] {
        let zeros = empty_hashes();
        std::array::from_fn(|i| {
            let depth = i + 1;
            if (self.size >> depth) % 2 == 1 {
                self.filled[depth]
            } else {
                zeros[depth]
            }
        })
    }
}

/// The root after putting `pair` at leaves `start` and `start + 1` of a tree whose insertion path
/// is `path`.
pub(crate) fn root_after_insert(pair: [Fr; 2], start: u64, path: &[Fr; TREE_DEPTH - 1]) -> Fr {
    let mut node = poseidon(&pair);
    let slot = start >> 1;
    for (depth, sibling) in path.iter().enumerate() {
        node = if (slot >> depth) & 1 == 1 {
            poseidon(&[*sibling, node])
        } else {
            poseidon(&[node, *sibling])
        };
    }
    node
}

/// The root a leaf at `index` with sibling `path` implies.
pub(crate) fn root_from_path(leaf: Fr, index: u64, path: &[Fr; TREE_DEPTH]) -> Fr {
    let mut node = leaf;
    for (depth, sibling) in path.iter().enumerate() {
        node = if (index >> depth) & 1 == 1 {
            poseidon(&[*sibling, node])
        } else {
            poseidon(&[node, *sibling])
        };
    }
    node
}

/// The serialized form of an [`IncTree`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncTreeState {
    pub size: u64,
    pub root: U256,
    pub filled: Vec<U256>,
    pub tracked: BTreeMap<u64, Vec<U256>>,
}

impl From<&IncTree> for IncTreeState {
    fn from(tree: &IncTree) -> Self {
        Self {
            size: tree.size,
            root: word(tree.root),
            filled: tree.filled.iter().copied().map(word).collect(),
            tracked: tree
                .tracked
                .iter()
                .map(|(index, path)| {
                    (
                        *index,
                        path.iter().map(|x| word(x.unwrap_or_default())).collect(),
                    )
                })
                .collect(),
        }
    }
}

impl TryFrom<IncTreeState> for IncTree {
    type Error = TreeError;

    fn try_from(state: IncTreeState) -> Result<Self, Self::Error> {
        let field = |x: U256| fr(x).ok_or(TreeError::InvalidState);
        let array = |xs: Vec<U256>| -> Result<[Fr; TREE_DEPTH], TreeError> {
            xs.into_iter()
                .map(field)
                .collect::<Result<Vec<_>, _>>()?
                .try_into()
                .map_err(|_| TreeError::InvalidState)
        };
        if state.size > 1 << TREE_DEPTH {
            return Err(TreeError::InvalidState);
        }
        let tracked = state
            .tracked
            .into_iter()
            .map(|(index, path)| Ok((index, array(path)?.map(Some))))
            .collect::<Result<_, TreeError>>()?;
        Ok(Self {
            size: state.size,
            root: field(state.root)?,
            filled: array(state.filled)?,
            tracked,
        })
    }
}
