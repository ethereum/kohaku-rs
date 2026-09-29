use std::{
    ops::{Bound, Range, RangeBounds},
    sync::Arc,
};

use thiserror::Error;

use crate::{
    abis::tornado::Tornado::{Deposit, Withdrawal},
    pool::Pool,
};

/// Generic syncer interface.
#[cfg_attr(native, async_trait::async_trait)]
#[cfg_attr(wasm, async_trait::async_trait(?Send))]
pub trait SyncerBackend: Send + Sync {
    /// See [`Syncer::latest_block`].
    async fn latest_block(&self, pool: &Pool) -> Result<u64, SyncerError>;

    /// See [`Syncer::sync`].
    ///
    /// `from_block` and `to_block` form a half-open range. Backends are expected to
    /// clamp this range to what they can serve.
    async fn sync(
        &self,
        pool: &Pool,
        from_block: u64,
        to_block: u64,
    ) -> Result<Synced, SyncerError>;
}

/// A syncer for tornadocash.
///
/// Syncers are used to fetch events from the chain to build a local pool representation.
#[derive(Clone)]
pub struct Syncer(Arc<dyn SyncerBackend>);

/// A set of events emitted by a pool within a given block range.
pub struct Synced {
    /// The half-open block range these events cover.
    pub range: Range<u64>,
    /// The events the pool emitted within `range`.
    pub events: Vec<SyncEvent>,
}

pub enum SyncEvent {
    Deposit(Deposit),
    Withdrawal(Withdrawal),
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SyncerError {
    #[error(transparent)]
    Other(Box<dyn std::error::Error + Send + Sync>),
}

impl Syncer {
    pub fn new(syncer: impl SyncerBackend + 'static) -> Self {
        Self(Arc::new(syncer))
    }

    /// Returns the latest block accessible by the syncer for the given `pool`.
    ///
    /// # Errors
    /// Returns an error if the syncer fails to fetch the latest block.
    pub async fn latest_block(&self, pool: &Pool) -> Result<u64, SyncerError> {
        self.0.latest_block(pool).await
    }

    /// Returns the events emitted by the given `pool` within the given block `range`.
    ///
    /// # Errors
    /// Returns an error if the syncer fails to fetch the events.
    pub async fn sync(
        &self,
        pool: &Pool,
        range: impl RangeBounds<u64>,
    ) -> Result<Synced, SyncerError> {
        let range = block_range(range);
        self.0.sync(pool, range.start, range.end).await
    }
}

impl<T: SyncerBackend + 'static> From<T> for Syncer {
    fn from(syncer: T) -> Self {
        Self::new(syncer)
    }
}

impl SyncerError {
    pub fn other<E: std::error::Error + Send + Sync + 'static>(err: E) -> Self {
        Self::Other(Box::new(err))
    }
}

/// Normalizes a block range into a half-open range.
fn block_range(range: impl RangeBounds<u64>) -> Range<u64> {
    let start = match range.start_bound() {
        Bound::Included(&block) => block,
        Bound::Excluded(&block) => block.saturating_add(1),
        Bound::Unbounded => 0,
    };
    let end = match range.end_bound() {
        Bound::Included(&block) => block.saturating_add(1),
        Bound::Excluded(&block) => block,
        Bound::Unbounded => u64::MAX,
    };

    start..end.max(start)
}
