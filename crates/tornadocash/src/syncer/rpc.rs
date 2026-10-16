use std::time::Duration;

use alloy::{providers::Provider, rpc::types::Filter};
use kohaku_rpc_provider::StreamedEvent;
use tracing::{info, warn};

use crate::{
    pool::Pool,
    syncer::{Snapshot, SyncEvent, Syncer, SyncerError},
};

/// A syncer that reads from an Ethereum JSON-RPC provider
#[derive(Clone)]
pub struct RpcSyncer<P: Provider> {
    provider: P,
    //? Unset options fall back to `StreamedEvent`'s defaults.
    chunk_size: Option<u64>,
    concurrent: Option<usize>,
    backoff: Option<(Duration, Duration)>,
    max_retries: Option<u32>,
}

#[derive(Debug, thiserror::Error)]
enum RpcSyncerError {
    #[error("Error decoding log: {0}")]
    LogDecodeError(#[from] alloy::sol_types::Error),
    #[error("RPC error: {0}")]
    RpcError(#[from] alloy::transports::RpcError<alloy::transports::TransportErrorKind>),
    #[error("Field conversion error: {0}")]
    Field(#[from] crate::field::NotInRangeError),
}

impl<P: Provider> RpcSyncer<P> {
    pub fn new(provider: P) -> Self {
        Self {
            provider,
            chunk_size: None,
            concurrent: None,
            backoff: None,
            max_retries: None,
        }
    }

    /// Sets the initial chunk size in blocks for `eth_getLogs` calls. See
    /// [`StreamedEvent::chunk_size`].
    #[must_use]
    pub fn with_chunk_size(mut self, chunk_size: u64) -> Self {
        self.chunk_size = Some(chunk_size);
        self
    }

    /// Sets the maximum number of concurrent `eth_getLogs` calls. See
    /// [`StreamedEvent::concurrent`].
    #[must_use]
    pub fn with_concurrent(mut self, max_concurrent: usize) -> Self {
        self.concurrent = Some(max_concurrent);
        self
    }

    /// Sets the rate-limit pause bounds. See [`StreamedEvent::backoff`].
    #[must_use]
    pub fn with_backoff(mut self, base_delay: Duration, max_delay: Duration) -> Self {
        self.backoff = Some((base_delay, max_delay));
        self
    }

    /// Sets how many consecutive rate limits a single call tolerates. See
    /// [`StreamedEvent::max_retries`].
    #[must_use]
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = Some(max_retries);
        self
    }
}

#[cfg_attr(native, async_trait::async_trait)]
#[cfg_attr(wasm, async_trait::async_trait(?Send))]
impl<P: Provider + Clone> Syncer for RpcSyncer<P> {
    async fn sync_range(
        &self,
        pool: &Pool,
        from_block: u64,
        to_block: u64,
    ) -> Result<Snapshot, SyncerError> {
        Ok(self
            .sync(pool, from_block, to_block)
            .await
            .map_err(SyncerError::other)?)
    }
}

impl<P: Provider + Clone> RpcSyncer<P> {
    async fn sync(
        &self,
        pool: &Pool,
        from_block: u64,
        to_block: u64,
    ) -> Result<Snapshot, RpcSyncerError> {
        let range = self.block_range(pool, from_block, to_block).await?;
        info!("Syncing from {} to {}", range.start, range.end);

        if range.is_empty() {
            return Ok(Snapshot::new(pool.clone(), range, Vec::new()));
        }

        //? `eth_getLogs` treats both block bounds as inclusive
        let filter = Filter::new()
            .address(pool.address)
            .from_block(range.start)
            .to_block(range.end - 1);
        let logs = self.streamed_event(filter).query().await?;

        let mut events = Vec::new();
        for log in logs {
            match SyncEvent::try_from_log(&log) {
                Some(decoded) => events.push(decoded),
                None => {
                    warn!("Unknown event with topic {:?}", log.topic0());
                }
            }
        }

        info!("Synced {} events", events.len());
        Ok(Snapshot::new(pool.clone(), range, events))
    }

    /// Clamps the requested range to the blocks this syncer can serve: the pool's deployment
    /// block through the chain's latest block.
    async fn block_range(
        &self,
        pool: &Pool,
        from_block: u64,
        to_block: u64,
    ) -> Result<std::ops::Range<u64>, RpcSyncerError> {
        let from = from_block.max(pool.deployed_block);
        //? `latest_block` is an inclusive block number, `to_block` an exclusive bound.
        let latest = self.provider.get_block_number().await?.saturating_add(1);
        let to = to_block.min(latest).max(from);

        Ok(from..to)
    }

    /// Builds a [`StreamedEvent`] for `filter` with this syncer's options.
    fn streamed_event(&self, filter: Filter) -> StreamedEvent<P> {
        let mut events = StreamedEvent::new(self.provider.clone(), filter);
        if let Some(chunk_size) = self.chunk_size {
            events = events.chunk_size(chunk_size);
        }
        if let Some(max_concurrent) = self.concurrent {
            events = events.concurrent(max_concurrent);
        }
        if let Some((base_delay, max_delay)) = self.backoff {
            events = events.backoff(base_delay, max_delay);
        }
        if let Some(max_retries) = self.max_retries {
            events = events.max_retries(max_retries);
        }
        events
    }
}
