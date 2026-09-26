use crate::{element::Element, hasher::Hasher};

/// A Merkle proof for a leaf element in a Merkle tree of depth `DEPTH` and arity `ARITY`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MerkleProof<const DEPTH: usize, const ARITY: usize, E> {
    pub leaf: E,
    /// The full child row at each level, including the node on the path itself.
    pub siblings: [[E; ARITY]; DEPTH],
    /// The index of the node within its parent's children at each level, in `0..ARITY`.
    pub path: [u8; DEPTH],
    pub root: E,
}

impl<const DEPTH: usize, const ARITY: usize, E: Element> MerkleProof<DEPTH, ARITY, E> {
    pub fn new(leaf: E, siblings: [[E; ARITY]; DEPTH], path: [u8; DEPTH], root: E) -> Self {
        Self {
            leaf,
            siblings,
            path,
            root,
        }
    }
}

impl<const DEPTH: usize, const ARITY: usize, E: Element + Eq> MerkleProof<DEPTH, ARITY, E> {
    /// Verifies the Merkle inclusion proof.
    pub fn verify<H: Hasher<ARITY, E>>(&self) -> bool {
        let mut hash = self.leaf.clone();
        for (level, &sibling_idx) in self.path.iter().enumerate() {
            let mut children = self.siblings[level].clone();
            children[sibling_idx as usize] = hash;
            hash = H::hash(children);
        }
        hash == self.root
    }
}
