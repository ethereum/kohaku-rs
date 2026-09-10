use std::{ops::Range, sync::Arc};

use thiserror::Error;

use crate::{
    abis::tornado::Tornado::{Deposit, Withdrawal},
    provider::pool::Pool,
};

/// Generic syncer interface.
#[async_trait::async_trait]
pub trait SyncerBackend: Send + Sync {
    /// See [`Syncer::latest_block`].
    async fn latest_block(&self, pool: &Pool) -> Result<u64, SyncerError>;

    /// See [`Syncer::sync`].
    async fn sync(
        &self,
        pool: &Pool,
        from_block: u64,
        to_block: u64,
    ) -> Result<Vec<SyncEvent>, SyncerError>;
}

/// A syncer for tornadocash.
///
/// Syncers are used to fetch events from the chain to build a local pool representation.
#[derive(Clone)]
pub struct Syncer(Arc<dyn SyncerBackend>);

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

    /// Returns the events emitted by the given `pool` for the given block `range`.
    ///
    /// # Errors
    /// Returns an error if the syncer fails to fetch the events.
    pub async fn sync(
        &self,
        pool: &Pool,
        range: Range<u64>,
    ) -> Result<Vec<SyncEvent>, SyncerError> {
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
