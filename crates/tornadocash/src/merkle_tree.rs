use std::sync::OnceLock;

use alloy::primitives::keccak256;
use ark_bn254::Fr;
use ark_ff::{BigInt, PrimeField};
use kohaku_merkle_tree::{MerkleTree, MerkleTreeError, hasher::Hasher};
use ruint::{aliases::U256, uint};

use crate::{crypto::mimc::mimc_sponge_hash, indexer::syncer::SyncEvent};

pub type TcMerkleTree = MerkleTree<20, TcMerkleTreeHasher>;

#[async_trait::async_trait]
pub trait TcMerkleTreeExt {
    /// Splices the given events into the Merkle tree.
    async fn splice_events(&self, events: &[SyncEvent]) -> Result<(), MerkleTreeError>;
}

#[derive(Copy, Clone)]
pub struct TcMerkleTreeHasher;

const FIELD_SIZE: U256 =
    uint!(21888242871839275222246405745257275088548364400416034343698204186575808495617_U256);

impl Hasher for TcMerkleTreeHasher {
    fn hash(l: U256, r: U256) -> U256 {
        let l: Fr = BigInt::from(l).into();
        let r: Fr = BigInt::from(r).into();
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

#[async_trait::async_trait]
impl TcMerkleTreeExt for TcMerkleTree {
    async fn splice_events(&self, events: &[SyncEvent]) -> Result<(), MerkleTreeError> {
        let first_leaf_index = events.iter().find_map(|e| match e {
            SyncEvent::Deposit(d) => Some(d.leafIndex),
            _ => None,
        });
        let commitments: Vec<U256> = events
            .iter()
            .filter_map(|e| match e {
                SyncEvent::Deposit(d) => Some(d.commitment.into()),
                _ => None,
            })
            .collect();

        if let Some(first_leaf_index) = first_leaf_index {
            self.splice(first_leaf_index as usize, &commitments).await?;
        }

        Ok(())
    }
}
