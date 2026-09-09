use std::{collections::HashMap, sync::Arc};

use ruint::aliases::U256;

use crate::{kv::KvStore, merkle_tree::kv::MerkleTreeStore};

mod kv;
pub mod proof;
pub mod tc;

pub struct MerkleTree<const D: usize, H: Hasher> {
    store: Arc<dyn KvStore>,
    phantom: std::marker::PhantomData<H>,
}

pub trait Hasher {
    /// Hashes two 32-byte arrays into a 32-byte hash.
    fn hash(a: U256, b: U256) -> U256;

    /// Returns the zero value for the hash function, which is used as a placeholder for empty
    /// leaves in the Merkle tree.
    fn zero() -> U256;
}

#[derive(Debug, thiserror::Error)]
pub enum MerkleTreeError {
    #[error("Merkle tree is full")]
    TreeFull,
    #[error("Missing leaf element")]
    MissingLeaf(U256),
    #[error("Index {0} is out of bounds")]
    IndexOutOfBounds(usize),
    #[error("Leaf at index {0} conflicts with an existing value")]
    LeafConflict(usize),
}

impl<const D: usize, H: Hasher> MerkleTree<D, H> {
    pub fn new(store: Arc<dyn KvStore>) -> Self {
        Self {
            store,
            phantom: std::marker::PhantomData,
        }
    }

    pub async fn root(&self) -> U256 {
        match self.store.node(D as u8, 0).await {
            Some(root) => root,
            None => zero_hashes::<H>(D)[D],
        }
    }

    /// Returns the inclusion proof for the given leaf.
    ///
    /// # Errors
    /// Returns an error if the leaf is not found in the tree.
    pub async fn leaf_proof(&self, leaf: U256) -> Result<proof::MerkleProof<D>, MerkleTreeError> {
        let len = self.store.leaf_count().await;
        for index in 0..len {
            if self.store.node(0, index).await == Some(leaf) {
                return self.proof(index as usize).await;
            }
        }

        Err(MerkleTreeError::MissingLeaf(leaf))
    }

    /// Returns the inclusion proof for the leaf at the given index.
    ///
    /// # Errors
    /// Returns an error if the index is out of bounds.
    pub async fn proof(&self, index: usize) -> Result<proof::MerkleProof<D>, MerkleTreeError> {
        let len = self.store.leaf_count().await as usize;
        if index >= len {
            return Err(MerkleTreeError::IndexOutOfBounds(index));
        }

        let zeros = zero_hashes::<H>(D);
        let mut path = [0u8; D];
        let mut siblings = [U256::ZERO; D];
        let mut idx = index;

        for level in 0..D {
            path[level] = (idx % 2) as u8;

            let sibling_idx = (idx ^ 1) as u64;
            siblings[level] = match self.store.node(level as u8, sibling_idx).await {
                Some(node) => node,
                None => zeros[level],
            };

            idx /= 2;
        }

        let element = self
            .store
            .node(0, index as u64)
            .await
            .ok_or(MerkleTreeError::IndexOutOfBounds(index))?;

        Ok(proof::MerkleProof {
            root: self.root().await,
            element,
            path,
            siblings,
        })
    }

    /// Inserts a single leaf at `index`. If a leaf already exists at that index, it must match
    /// `leaf`.
    ///
    /// # Errors
    /// Returns an error if the index is out of bounds or if the leaf conflicts with an existing
    /// value.
    pub async fn insert(&self, index: usize, leaf: U256) -> Result<(), MerkleTreeError> {
        self.splice(index, &[leaf]).await
    }

    /// Splices in `leaves` starting at at `index`. If any leaves already exist at those indices,
    /// they must match the corresponding values in `leaves`.
    ///
    /// # Errors
    /// Returns an error if the index is out of bounds or if any leaf conflicts with an existing
    /// value.
    pub async fn splice(&self, index: usize, leaves: &[U256]) -> Result<(), MerkleTreeError> {
        if leaves.is_empty() {
            return Ok(());
        }

        let len = self.store.leaf_count().await as usize;
        if index > len {
            return Err(MerkleTreeError::IndexOutOfBounds(index));
        }

        let overlap = (len - index).min(leaves.len());
        for (offset, &leaf) in leaves[..overlap].iter().enumerate() {
            if self.store.node(0, (index + offset) as u64).await != Some(leaf) {
                return Err(MerkleTreeError::LeafConflict(index + offset));
            }
        }

        let leaves = &leaves[overlap..];
        if leaves.is_empty() {
            return Ok(());
        }

        let capacity = 2usize.checked_pow(D as u32).unwrap_or(usize::MAX);
        let new_len = len + leaves.len();
        if new_len > capacity {
            return Err(MerkleTreeError::TreeFull);
        }

        let zeros = zero_hashes::<H>(D);
        let mut pending: HashMap<(u8, u64), U256> = HashMap::new();
        for (offset, &leaf) in leaves.iter().enumerate() {
            pending.insert((0, (len + offset) as u64), leaf);
        }

        let mut child_len = len;
        let mut child_new_len = new_len;
        for level in 1..=D {
            // Groups fully backed by real children before this write are already final; only the
            // (at most one) previously-incomplete trailing group and any brand-new groups need
            // recomputing.
            let start = child_len / 2;
            let end = child_new_len.div_ceil(2);

            for parent_idx in start..end {
                let mut children = [zeros[level - 1]; 2];
                for (k, child) in children.iter_mut().enumerate() {
                    let child_idx = (parent_idx * 2 + k) as u64;
                    *child = if let Some(&node) = pending.get(&((level - 1) as u8, child_idx)) {
                        node
                    } else if (child_idx as usize) < child_len {
                        self.store
                            .node((level - 1) as u8, child_idx)
                            .await
                            .unwrap_or(zeros[level - 1])
                    } else {
                        zeros[level - 1]
                    };
                }
                pending.insert(
                    (level as u8, parent_idx as u64),
                    H::hash(children[0], children[1]),
                );
            }

            child_len = child_len.div_ceil(2);
            child_new_len = end;
        }

        let nodes: Vec<(u8, u64, U256)> = pending
            .into_iter()
            .map(|((level, node_index), hash)| (level, node_index, hash))
            .collect();
        self.store.commit(new_len as u64, &nodes).await;

        Ok(())
    }
}

/// Computes the hash of a fully-empty subtree at each height from `0` to `levels` (inclusive),
/// where height `0` is the hasher's zero leaf value.
fn zero_hashes<H: Hasher>(levels: usize) -> Vec<U256> {
    let mut zeros = Vec::with_capacity(levels + 1);
    zeros.push(H::zero());
    for i in 1..=levels {
        zeros.push(H::hash(zeros[i - 1], zeros[i - 1]));
    }
    zeros
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kv::MemoryKvStore;

    struct TestHasher;

    impl Hasher for TestHasher {
        fn hash(a: U256, b: U256) -> U256 {
            a ^ b.rotate_left(1)
        }

        fn zero() -> U256 {
            U256::ZERO
        }
    }

    type TestTree<const D: usize> = MerkleTree<D, TestHasher>;

    fn tree<const D: usize>() -> TestTree<D> {
        MerkleTree::new(Arc::new(MemoryKvStore::default()))
    }

    fn leaf(byte: u8) -> U256 {
        U256::from_le_bytes([byte; 32])
    }

    #[tokio::test]
    async fn empty_tree_root_is_deterministic() {
        let a = tree::<3>();
        let b = tree::<3>();
        assert_eq!(a.root().await, b.root().await);
    }

    #[tokio::test]
    async fn insert_and_splice_produce_same_root() {
        let leaves = [leaf(1), leaf(2), leaf(3)];

        let inserted = tree::<3>();
        for (i, &l) in leaves.iter().enumerate() {
            inserted.insert(i, l).await.unwrap();
        }

        let spliced = tree::<3>();
        spliced.splice(0, &leaves).await.unwrap();

        assert_eq!(inserted.root().await, spliced.root().await);
    }

    #[tokio::test]
    async fn partial_overlap_splice_extends_correctly() {
        let leaves = [leaf(1), leaf(2), leaf(3)];
        let full = tree::<3>();
        full.splice(0, &leaves).await.unwrap();

        // Simulate replaying from a point where leaf(1) was already committed.
        let replayed = tree::<3>();
        replayed.insert(0, leaf(1)).await.unwrap();
        replayed.splice(0, &leaves).await.unwrap();

        assert_eq!(full.root().await, replayed.root().await);
    }

    #[tokio::test]
    async fn reinserting_the_same_leaf_is_a_no_op() {
        let tree = tree::<3>();
        tree.insert(0, leaf(1)).await.unwrap();
        let root_before = tree.root().await;

        tree.insert(0, leaf(1)).await.unwrap();
        assert_eq!(tree.root().await, root_before);
    }

    #[tokio::test]
    async fn reinserting_a_different_leaf_conflicts() {
        let tree = tree::<3>();
        tree.insert(0, leaf(1)).await.unwrap();

        assert!(matches!(
            tree.insert(0, leaf(2)).await,
            Err(MerkleTreeError::LeafConflict(0))
        ));
    }

    #[tokio::test]
    async fn inserting_past_the_end_errors() {
        let tree = tree::<3>();

        assert!(matches!(
            tree.insert(1, leaf(1)).await,
            Err(MerkleTreeError::IndexOutOfBounds(1))
        ));
    }

    #[tokio::test]
    async fn leaf_proof_matches_recomputed_root() {
        let leaves = [leaf(1), leaf(2), leaf(3)];
        let tree = tree::<3>();
        tree.splice(0, &leaves).await.unwrap();

        let proof = tree.leaf_proof(leaf(2)).await.unwrap();
        assert_eq!(proof.root, tree.root().await);
        assert_eq!(proof.element, leaf(2));

        let mut current = proof.element;
        for level in 0..3 {
            let sibling = proof.siblings[level];
            current = if proof.path[level] == 0 {
                TestHasher::hash(current, sibling)
            } else {
                TestHasher::hash(sibling, current)
            };
        }
        assert_eq!(current, proof.root);
    }

    #[tokio::test]
    async fn leaf_proof_missing_leaf_errors() {
        let tree = tree::<3>();
        tree.insert(0, leaf(1)).await.unwrap();

        assert!(matches!(
            tree.leaf_proof(leaf(9)).await,
            Err(MerkleTreeError::MissingLeaf(_))
        ));
    }

    #[tokio::test]
    async fn proof_out_of_bounds_errors() {
        let tree = tree::<3>();
        tree.insert(0, leaf(1)).await.unwrap();

        assert!(matches!(
            tree.proof(5).await,
            Err(MerkleTreeError::IndexOutOfBounds(5))
        ));
    }

    #[tokio::test]
    async fn splice_beyond_capacity_errors() {
        let tree = tree::<2>(); // capacity = 2^2 = 4
        tree.splice(0, &[leaf(1), leaf(2), leaf(3), leaf(4)])
            .await
            .unwrap();

        assert!(matches!(
            tree.insert(4, leaf(5)).await,
            Err(MerkleTreeError::TreeFull)
        ));
    }
}
