use alloy::primitives::{B256, U256};
use kohaku_kv_store::{Store, backend::StoreError};
use thiserror::Error;
use tracing::info;

use crate::{
    indexer::{
        indexer_store::IndexerStoreExt,
        syncer::{SyncEvent, Syncer, SyncerError},
        verifier::{Verifier, VerifierError},
    },
    merkle_tree::TcMerkleTree,
    pool::Pool,
};

pub mod chained;
mod indexer_store;
pub mod remote;
pub mod rpc;
#[cfg(feature = "saga-sync")]
pub mod saga_sync;
pub mod syncer;
pub mod verifier;

/// Number of blocks to re-sync behind the last synced block, to recover from shallow reorgs.
const REORG_MARGIN: u64 = 32;

/// An indexer for a single tornadocash pool.
///
/// The indexer syncs the pool's events and maintains a local merkle tree of the pool's commitments.
#[derive(Clone)]
pub struct Indexer {
    pool: Pool,
    store: Store,
    syncer: Syncer,
    verifier: Verifier,
    tree: TcMerkleTree,
}

#[derive(Debug, Error)]
pub enum IndexerError {
    #[error("Syncer error: {0}")]
    Syncer(#[from] SyncerError),
    #[error("Verifier error: {0}")]
    Verifier(#[from] VerifierError),
    #[error("Merkle tree error: {0}")]
    MerkleTree(#[from] kohaku_merkle_tree::MerkleTreeError),
    #[error("Store error: {0}")]
    Store(#[from] StoreError),
}

impl Indexer {
    /// Creates a new indexer for the given pool, using the provided syncer and verifier.
    #[must_use]
    pub fn new(pool: Pool, store: Store, syncer: Syncer, verifier: Verifier) -> Self {
        let tree = TcMerkleTree::new(store.clone());

        Self {
            pool,
            store,
            syncer,
            verifier,
            tree,
        }
    }

    #[must_use]
    pub fn pool(&self) -> Pool {
        self.pool.clone()
    }

    #[must_use]
    pub fn tree(&self) -> &TcMerkleTree {
        &self.tree
    }

    /// Returns the leaf index of a given commitment if it exists.
    #[must_use]
    pub async fn commitment(&self, commitment: U256) -> Result<Option<u32>, IndexerError> {
        Ok(self.store.get_commitment(commitment).await?)
    }

    /// Returns `Some` if the given nullifier hash exists.
    #[must_use]
    pub async fn is_spent(&self, nullifier_hash: B256) -> Result<bool, IndexerError> {
        Ok(self.store.get_nullifier_hash(nullifier_hash).await?)
    }

    /// Syncs the indexer to the latest block.
    ///
    /// # Errors
    /// Returns an error if the syncer fails, or if the indexer fails to save its state to the
    /// database.
    pub async fn sync(&self) -> Result<(), IndexerError> {
        let latest = self.syncer.latest_block(&self.pool).await?;
        self.sync_to(latest).await?;

        Ok(self
            .verifier
            .verify(&self.pool, self.tree.root().await?)
            .await?)
    }

    /// Syncs the indexer to the given block.
    #[tracing::instrument(skip(self))]
    async fn sync_to(&self, to_block: u64) -> Result<(), IndexerError> {
        let latest = self.store.latest_block().await?;

        let from_block = latest
            .saturating_sub(REORG_MARGIN)
            .max(self.pool.deployed_block);
        if from_block >= to_block {
            info!("Already synced to block {}", latest);
            return Ok(());
        }
        info!("Syncing from {} to {}", from_block, to_block);

        let events = self.syncer.sync(&self.pool, from_block..to_block).await?;
        info!("Synced {} events", events.len());

        let mut leaves = Vec::new();
        let mut nullifier_hashes = Vec::new();
        for event in events {
            match event {
                SyncEvent::Deposit(d) => {
                    leaves.push((d.leafIndex, d.commitment.into()));
                }
                SyncEvent::Withdrawal(w) => nullifier_hashes.push(w.nullifierHash.into()),
            }
        }

        if !leaves.is_empty() {
            let start = leaves[0].0 as usize;
            let leaf_values: Vec<_> = leaves.iter().map(|(_, val)| *val).collect();

            self.tree.splice(start, &leaf_values).await?;
        }

        self.store
            .commit(to_block, &leaves, &nullifier_hashes)
            .await?;
        Ok(())
    }
}
