use tracing::info;

use crate::{
    indexer::syncer::{SyncEvent, Syncer, SyncerBackend, SyncerError},
    provider::pool::Pool,
};

/// Helper syncer that chains multiple UTXO syncers together.
///
/// Syncers are queried in the order they are added, and the sync range is adjusted based on the
/// latest block of each syncer.
#[derive(Default)]
pub struct ChainedSyncer {
    syncers: Vec<Syncer>,
}

#[derive(Debug, thiserror::Error)]
#[error("no syncer available to cover blocks {from}..{to}")]
struct NoSyncerAvailable {
    from: u64,
    to: u64,
}

impl ChainedSyncer {
    #[must_use]
    pub fn new() -> Self {
        Self {
            syncers: Vec::new(),
        }
    }

    /// Adds a syncer to the chain. Syncers are queried in the order they are added.
    #[must_use]
    pub fn then<S: SyncerBackend + 'static>(mut self, syncer: S) -> Self {
        self.syncers.push(syncer.into());
        self
    }
}

#[async_trait::async_trait]
impl SyncerBackend for ChainedSyncer {
    async fn latest_block(&self, pool: &Pool) -> Result<u64, SyncerError> {
        let mut max_block = 0u64;
        for syncer in &self.syncers {
            match syncer.latest_block(pool).await {
                Ok(block) => {
                    max_block = max_block.max(block);
                }
                Err(e) => {
                    tracing::warn!("Syncer failed to get latest block: {}", e);
                }
            }
        }
        Ok(max_block)
    }

    async fn sync(
        &self,
        pool: &Pool,
        from_block: u64,
        to_block: u64,
    ) -> Result<Vec<SyncEvent>, SyncerError> {
        info!("Syncing from {} to {}", from_block, to_block);
        let mut current_from = from_block;
        let mut all_events = Vec::new();

        for (i, syncer) in self.syncers.iter().enumerate() {
            if current_from >= to_block {
                break;
            }

            let syncer_latest = match syncer.latest_block(pool).await {
                Ok(block) => block,
                Err(e) => {
                    tracing::warn!("Syncer {} failed to get latest block: {}", i, e);
                    continue;
                }
            };
            if syncer_latest <= current_from {
                continue;
            }

            let range_end = syncer_latest.min(to_block);
            match syncer.sync(pool, current_from..range_end).await {
                Ok(events) => {
                    all_events.extend(events);
                    current_from = range_end;
                }
                Err(e) => {
                    tracing::warn!(
                        "Syncer {} failed for range {}..{}: {}, trying next syncer",
                        i,
                        current_from,
                        range_end,
                        e
                    );
                    // Leave `current_from` unchanged so the next syncer retries this range.
                }
            }
        }

        if current_from < to_block {
            return Err(SyncerError::other(NoSyncerAvailable {
                from: current_from,
                to: to_block,
            }));
        }

        Ok(all_events)
    }
}
