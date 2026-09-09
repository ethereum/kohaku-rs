use std::sync::Arc;

use thiserror::Error;
use tracing::info;

use crate::{
    indexer::{
        kv::IndexerStore,
        syncer::{SyncEvent, Syncer, SyncerError},
        verifier::{Verifier, VerifierError},
    },
    kv::KvStore,
    merkle_tree::tc::TcMerkleTree,
    provider::pool::Pool,
};

pub mod chained;
mod kv;
pub mod remote;
pub mod rpc;
pub mod syncer;
pub mod verifier;

pub struct Indexer {
    pool: Pool,
    syncer: Arc<dyn Syncer>,
    verifier: Arc<dyn Verifier>,
    tree: TcMerkleTree,
    store: Arc<dyn KvStore>,
}

#[derive(Debug, Error)]
pub enum IndexerError {
    #[error("Syncer error: {0}")]
    Syncer(#[from] SyncerError),
    #[error("Verifier error: {0}")]
    Verifier(#[from] VerifierError),
    #[error("Unknown pool: amount={0}, symbol={1}, chain_id={2}")]
    UnknownPool(String, String, u64),
    #[error("Merkle tree error: {0}")]
    MerkleTree(#[from] crate::merkle_tree::MerkleTreeError),
}

impl Indexer {
    /// Creates a new indexer for the given pool, using the provided syncer and verifier.
    ///
    /// # Errors
    /// Returns an error if the indexer state cannot be loaded from the database.
    pub fn new(
        store: Arc<dyn KvStore>,
        pool: Pool,
        syncer: Arc<dyn Syncer>,
        verifier: Arc<dyn Verifier>,
    ) -> Result<Self, IndexerError> {
        let tree = TcMerkleTree::new(store.clone());

        Ok(Self {
            pool,
            syncer,
            verifier,
            tree,
            store,
        })
    }

    #[must_use]
    pub fn pool(&self) -> &Pool {
        &self.pool
    }

    #[must_use]
    pub fn tree(&self) -> &TcMerkleTree {
        &self.tree
    }

    /// Verifies that the current root is known on-chain
    ///
    /// # Errors
    /// Returns an error if the root is invalid or the verifier fails to verify the root.
    pub async fn verify(&self) -> Result<(), IndexerError> {
        Ok(self
            .verifier
            .verify(&self.pool, self.tree.root().await)
            .await?)
    }

    /// Syncs the indexer to the latest block.
    ///
    /// # Errors
    /// Returns an error if the syncer fails, or if the indexer fails to save its state to the
    /// database.
    pub async fn sync(&mut self) -> Result<(), IndexerError> {
        let latest = self.syncer.latest_block(&self.pool).await?;
        self.sync_to(latest).await
    }

    /// Syncs the indexer to the given block.
    ///
    /// # Errors
    /// Returns an error if the syncer fails, or if the indexer fails to save its state to the
    /// database.
    #[tracing::instrument(skip(self))]
    pub async fn sync_to(&mut self, to_block: u64) -> Result<(), IndexerError> {
        let latest = self.store.latest_block().await;

        let from_block = latest.max(self.pool.deployed_block);
        if from_block >= to_block {
            info!("Already synced to block {}", latest);
            return Ok(());
        }
        info!("Syncing from {} to {}", from_block, to_block);

        let events = self.syncer.sync(&self.pool, from_block, to_block).await?;
        info!("Synced {} events", events.len());

        let mut leaves = Vec::new();
        let mut nullifiers = Vec::new();
        for event in events {
            match event {
                SyncEvent::Deposit(d) => {
                    leaves.push((d.leafIndex, d.commitment.into()));
                }
                SyncEvent::Withdrawal(w) => {
                    nullifiers.push(w.nullifierHash.into());
                }
            }
        }

        if !leaves.is_empty() {
            let start = leaves[0].0 as usize;
            let leaves: Vec<_> = leaves.into_iter().map(|(_, val)| val).collect();

            self.tree.splice(start, &leaves).await?;
        }

        self.store.commit(to_block, &nullifiers).await;
        Ok(())
    }
}
