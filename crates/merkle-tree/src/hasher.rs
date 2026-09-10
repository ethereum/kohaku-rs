use ruint::aliases::U256;

/// A merkle tree hash function.
///
/// The hash function should be collision-resistant and deterministic. It's
/// used to hash a pair of nodes into a parent node, and to provide the zero
/// value for empty leaves.
pub trait Hasher {
    /// Hashes two 32-byte arrays into a 32-byte hash.
    fn hash(a: U256, b: U256) -> U256;

    /// Returns the zero value for the hash function, which is used as a placeholder for empty
    /// leaves in the Merkle tree.
    fn zero() -> U256;
}
