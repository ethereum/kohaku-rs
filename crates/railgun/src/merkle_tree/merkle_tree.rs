use std::fmt::Display;

use alloy::primitives::utils::keccak256_cached;
use kohaku_crypto::poseidon_hash;
use kohaku_merkle_tree::Hasher;
use ruint::aliases::U256;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::crypto::railgun_zero::SNARK_PRIME;

pub const TREE_DEPTH: usize = 16;
pub const TOTAL_LEAVES: u32 = 1 << TREE_DEPTH;

pub type MerkleTree = NumberedMerkleTree<RailgunHasher>;
pub type MerkleProof = kohaku_merkle_tree::MerkleProof<TREE_DEPTH, 2, U256>;
pub type MerkleTreeError = kohaku_merkle_tree::MerkleTreeError;

type InnerTree<H> = kohaku_merkle_tree::MerkleTree<TREE_DEPTH, 2, U256, H>;

/// A Railgun Merkle tree tagged with its tree number.
#[derive(Clone)]
pub struct NumberedMerkleTree<H: Hasher<2, U256>> {
    number: u32,
    inner: InnerTree<H>,
}

/// Caller-owned tree checkpoint. Authenticate it before restoring cached nodes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MerkleTreeState {
    pub number: u32,
    pub leaves: Vec<U256>,
    /// Computed levels 1..=TREE_DEPTH. Older leaves-only checkpoints rebuild
    /// once and acquire this cache on their next successful save.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_nodes: Option<Vec<Vec<U256>>>,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct MerkleRoot(U256);

#[derive(Debug, Clone, Copy)]
pub struct RailgunHasher;

impl<H: Hasher<2, U256>> NumberedMerkleTree<H> {
    pub fn new(number: u32) -> Self {
        Self {
            number,
            inner: InnerTree::new(),
        }
    }

    pub fn from_state(state: MerkleTreeState) -> Result<Self, MerkleTreeError> {
        let inner = match state.cached_nodes {
            Some(nodes) => InnerTree::from_trusted_nodes(state.leaves, nodes)?,
            None => InnerTree::from_leaves(&state.leaves)?,
        };
        Ok(Self {
            number: state.number,
            inner,
        })
    }

    pub fn zero() -> U256 {
        H::zero()
    }

    pub fn number(&self) -> u32 {
        self.number
    }

    pub fn root(&self) -> Result<MerkleRoot, MerkleTreeError> {
        Ok(self.inner.root().into())
    }

    pub fn leaves(&self) -> &[U256] {
        self.inner.leaves()
    }

    pub fn leaves_len(&self) -> usize {
        self.inner.leaves().len()
    }

    pub fn state(&self) -> MerkleTreeState {
        MerkleTreeState {
            number: self.number,
            leaves: self.inner.leaves().to_vec(),
            cached_nodes: Some(self.inner.nodes()[1..].to_vec()),
        }
    }

    pub fn generate_proof(&self, element: U256) -> Result<MerkleProof, MerkleTreeError> {
        self.inner.leaf_proof(&element)
    }

    /// Inclusion proof for the leaf at `index`, without scanning for the value.
    pub fn generate_proof_at(&self, index: usize) -> Result<MerkleProof, MerkleTreeError> {
        self.inner.proof(index)
    }

    /// Inserts leaves starting at the given position, replacing any already there.
    pub fn insert_leaves(
        &mut self,
        leaves: &[U256],
        start_position: usize,
    ) -> Result<(), MerkleTreeError> {
        self.inner.splice(start_position, leaves)
    }
}

/// Bit-packed leaf position of a proof, as the circuits expect it.
pub fn proof_indices(proof: &MerkleProof) -> U256 {
    proof
        .path
        .iter()
        .enumerate()
        .fold(U256::ZERO, |acc, (level, &bit)| {
            acc | (U256::from(bit) << level)
        })
}

impl Hasher<2, U256> for RailgunHasher {
    fn hash(children: [U256; 2]) -> U256 {
        poseidon_hash(&children).unwrap()
    }

    fn zero() -> U256 {
        let hash = U256::from_be_bytes(*keccak256_cached(b"Railgun"));
        hash % SNARK_PRIME
    }
}

impl MerkleRoot {
    pub fn new(root: U256) -> Self {
        MerkleRoot(root)
    }
}

impl From<U256> for MerkleRoot {
    fn from(value: U256) -> Self {
        MerkleRoot(value)
    }
}

impl From<MerkleRoot> for U256 {
    fn from(value: MerkleRoot) -> Self {
        value.0
    }
}

impl Display for MerkleRoot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:064x}", self.0)
    }
}

impl Serialize for MerkleRoot {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for MerkleRoot {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        let s = s.strip_prefix("0x").unwrap_or(&s);
        U256::from_str_radix(s, 16)
            .map(MerkleRoot)
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(all(test, native))]
mod tests {
    use ruint::uint;
    use tracing_test::traced_test;

    use super::*;

    #[test]
    fn test_railgun_merkle_tree_zero() {
        let zero = RailgunHasher::zero();
        let expected = uint!(
            2051258411002736885948763699317990061539314419500486054347250703186609807356_U256
        );
        assert_eq!(zero, expected);
    }

    #[test]
    #[traced_test]
    fn test_empty_merkleroot() {
        let tree = MerkleTree::new(0);
        let expected_root: MerkleRoot = uint!(
            9493149700940509817378043077993653487291699154667385859234945399563579865744_U256
        )
        .into();

        assert_eq!(tree.root().unwrap(), expected_root);
    }

    #[test]
    fn checkpoint_restores_cached_nodes_and_accepts_old_leaves_only_json() {
        let mut tree = MerkleTree::new(0);
        tree.insert_leaves(&[U256::from(1), U256::from(2), U256::from(3)], 0)
            .unwrap();
        let mut json = serde_json::to_value(tree.state()).unwrap();
        let cached = MerkleTree::from_state(serde_json::from_value(json.clone()).unwrap()).unwrap();
        assert_eq!(tree.root().unwrap(), cached.root().unwrap());
        assert_eq!(
            tree.generate_proof_at(1).unwrap(),
            cached.generate_proof_at(1).unwrap()
        );

        json.as_object_mut().unwrap().remove("cached_nodes");
        let old = MerkleTree::from_state(serde_json::from_value(json).unwrap()).unwrap();
        assert_eq!(tree.root().unwrap(), old.root().unwrap());
        assert!(old.state().cached_nodes.is_some());
    }

    #[test]
    fn test_proof_indices_packs_path_little_endian() {
        let index = 0b1000_0000_0010_0101usize;
        let path = std::array::from_fn(|level| ((index >> level) & 1) as u8);
        let proof = MerkleProof::new(U256::ZERO, Default::default(), path, U256::ZERO);

        assert_eq!(proof_indices(&proof), U256::from(index));
    }
}
