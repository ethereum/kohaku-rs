use ruint::aliases::U256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MerkleProof<const D: usize> {
    /// The root of the Merkle tree.
    pub root: U256,
    /// The leaf element.
    pub element: U256,
    /// Path from the root to the leaf.
    pub path: [u8; D],
    /// The siblings of the leaf element at each level.
    pub siblings: [U256; D],
}
