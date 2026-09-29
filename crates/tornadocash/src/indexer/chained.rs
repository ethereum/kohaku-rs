use tracing::info;

use crate::{
    indexer::syncer::{Synced, Syncer, SyncerBackend, SyncerError},
    pool::Pool,
};

/// Helper syncer that chains multiple UTXO syncers together.
///
/// Syncers are queried in the order they are added, each one picking up from
/// where the previous one left off.
#[derive(Default)]
pub struct ChainedSyncer {
    syncers: Vec<Syncer>,
}

impl ChainedSyncer {
    #[must_use]
    pub fn new() -> Self {
        Self {
            syncers: Vec::new(),
        }
    }

    /// Adds a syncer to the chain.
    #[must_use]
    pub fn then<S: SyncerBackend + 'static>(mut self, syncer: S) -> Self {
        self.syncers.push(syncer.into());
        self
    }
}

#[cfg_attr(native, async_trait::async_trait)]
#[cfg_attr(wasm, async_trait::async_trait(?Send))]
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
    ) -> Result<Synced, SyncerError> {
        info!("Syncing from {} to {}", from_block, to_block);

        //? Start at the pool's deployment block so a backend clamping the range up doesn't read
        //? as a gap below.
        let start = from_block.max(pool.deployed_block);
        let mut current = start;
        let mut events = Vec::new();

        for (i, syncer) in self.syncers.iter().enumerate() {
            if current >= to_block {
                break;
            }

            let synced = match syncer.sync(pool, current..to_block).await {
                Ok(synced) => synced,
                Err(e) => {
                    // Leave `current` unchanged so the next syncer retries this range.
                    tracing::warn!(
                        "Syncer {} failed for range {}..{}: {}, trying next syncer",
                        i,
                        current,
                        to_block,
                        e
                    );
                    continue;
                }
            };

            if synced.range.start > current {
                break;
            }

            events.extend(synced.events);
            current = current.max(synced.range.end);
        }

        Ok(Synced {
            range: start..current,
            events,
        })
    }
}
