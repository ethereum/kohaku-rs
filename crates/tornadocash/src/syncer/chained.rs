use std::sync::Arc;

use tracing::info;

use crate::{
    pool::Pool,
    syncer::{Synced, SyncerBackend, SyncerError},
};

/// Helper syncer that chains multiple UTXO syncers together.
///
/// Syncers are queried in the order they are added, each one picking up from
/// where the previous one left off.
#[derive(Default)]
pub struct ChainedSyncer {
    syncers: Vec<Arc<dyn SyncerBackend>>,
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
        self.syncers.push(Arc::new(syncer));
        self
    }
}

#[cfg_attr(native, async_trait::async_trait)]
#[cfg_attr(wasm, async_trait::async_trait(?Send))]
impl SyncerBackend for ChainedSyncer {
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

            let synced = match syncer.sync(pool, current, to_block).await {
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
