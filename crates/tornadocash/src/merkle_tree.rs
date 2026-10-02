use std::sync::OnceLock;

use alloy::primitives::keccak256;
use ark_bn254::Fr;
use ark_ff::{BigInt, PrimeField};
use kohaku_merkle_tree::{MerkleTreeError, hasher::Hasher};
use ruint::{aliases::U256, uint};

use crate::{
    crypto::mimc::mimc_sponge_hash,
    syncer::event::{Deposit, SyncEvent},
};

/// `MerkleTree` type used in Tornado Cash.
pub type MerkleTree = kohaku_merkle_tree::MerkleTree<20, 2, U256, TornadoHasher>;

/// `MerkleProof` type used in Tornado Cash.
pub type MerkleProof = kohaku_merkle_tree::proof::MerkleProof<20, 2, U256>;

/// `MerkleTree` extension trait for Tornado Cash.
pub trait MerkleTreeExt {
    /// Splices the given events into the Merkle tree.
    fn splice_events(&mut self, events: &[SyncEvent]) -> Result<(), MerkleTreeError>;
}

/// Hasher used in Tornado Cash Merkle tree.
///
/// Follows the hashing scheme used in Tornado Cash. Reference implementation:
/// [contracts/Classic/MerkleTreeWithHistory.sol](https://github.com/tornado-dao/tornado-contracts/blob/cc57528ae13e762c36ae715f37e018528d6e0605/contracts/Classic/MerkleTreeWithHistory.sol#L57)
#[derive(Copy, Clone)]
pub struct TornadoHasher;

const FIELD_SIZE: U256 =
    uint!(21888242871839275222246405745257275088548364400416034343698204186575808495617_U256);

impl Hasher<2, U256> for TornadoHasher {
    fn hash(children: [U256; 2]) -> U256 {
        let l: Fr = BigInt::from(children[0]).into();
        let r: Fr = BigInt::from(children[1]).into();
        mimc_sponge_hash(l, r).into_bigint().into()
    }

    fn zero() -> U256 {
        static ZERO: OnceLock<U256> = OnceLock::new();
        *ZERO.get_or_init(|| {
            let hash = keccak256(b"tornado");
            let hash_u256 = U256::from_be_bytes(*hash);
            hash_u256 % FIELD_SIZE
        })
    }
}

impl MerkleTreeExt for MerkleTree {
    fn splice_events(&mut self, events: &[SyncEvent]) -> Result<(), MerkleTreeError> {
        let deposits: Vec<&Deposit> = events
            .iter()
            .filter_map(|e| match e {
                SyncEvent::Deposit(d) => Some(d),
                SyncEvent::Withdrawal(_) => None,
            })
            .collect();

        let Some(first_leaf_index) = deposits.first().map(|d| d.leaf_index) else {
            return Ok(());
        };

        for (offset, d) in deposits.iter().enumerate() {
            let expected = first_leaf_index + offset as u32;
            if d.leaf_index != expected {
                return Err(MerkleTreeError::Other(format!(
                    "non-contiguous deposit events: expected leaf {expected}, got {}",
                    d.leaf_index
                )));
            }
        }

        let commitments: Vec<U256> = deposits.into_iter().map(|d| d.commitment.into()).collect();
        self.splice(first_leaf_index as usize, &commitments)
    }
}

#[cfg(test)]
mod tests {
    use std::array::from_fn;

    use ruint::uint;

    use super::*;

    #[test]
    fn zero_hash_is_deterministic() {
        // https://etherscan.io/address/0x910cbd523d972eb0a6f4cae4618ad62622b39dbf#readContract#F17
        let zero = TornadoHasher::zero();
        let expected = uint!(
            21663839004416932945382355908790599225266501822907911457504978515578255421292_U256
        );

        assert_eq!(zero, expected);
    }

    #[test]
    fn hash_is_deterministic() {
        // https://etherscan.io/address/0x910cbd523d972eb0a6f4cae4618ad62622b39dbf#readContract#F3
        let l = uint!(0x0000000000000000000000000000000000000000000000000000000000000001_U256);
        let r = uint!(0x0000000000000000000000000000000000000000000000000000000000000002_U256);

        let hash = TornadoHasher::hash([l, r]);
        let expected = uint!(
            19814528709687996974327303300007262407299502847885145507292406548098437687919_U256
        );

        assert_eq!(hash, expected);
    }

    #[test]
    fn splice_is_deterministic() {
        let mut tree = crate::merkle_tree::MerkleTree::new();

        let leaves: [U256; 10] = from_fn(|i| U256::from(i + 1));
        tree.splice(0, &leaves).unwrap();

        let root = tree.root().unwrap();
        let expected = uint!(
            15200063891796499502721825879098395282261979630510984497744981505913469850275_U256
        );
        assert_eq!(root, expected);
    }
}
