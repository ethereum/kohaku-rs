use ruint::aliases::U256;

/// A Merkle proof for a leaf element in a Merkle tree of depth `D`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MerkleProof<const D: usize> {
    pub leaf: U256,
    pub path: [u8; D],
    pub siblings: [U256; D],
    pub root: U256,
}
