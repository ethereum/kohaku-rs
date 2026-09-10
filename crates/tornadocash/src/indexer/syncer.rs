use thiserror::Error;

use crate::{
    abis::tornado::Tornado::{Deposit, Withdrawal},
    provider::pool::Pool,
};

/// A syncer for tornadocash.
#[async_trait::async_trait]
pub trait Syncer: Send + Sync {
    /// Returns the latest block accessible by the syncer for the given `pool`.
    async fn latest_block(&self, pool: &Pool) -> Result<u64, SyncerError>;

    /// Returns a list of all sync events between `from_block` and `to_block` for
    /// the given `pool`.
    async fn sync(
        &self,
        pool: &Pool,
        from_block: u64,
        to_block: u64,
    ) -> Result<Vec<SyncEvent>, SyncerError>;
}

pub enum SyncEvent {
    Deposit(Deposit),
    Withdrawal(Withdrawal),
}

#[derive(Debug, Error)]
pub enum SyncerError {
    #[error(transparent)]
    Other(Box<dyn std::error::Error + Send + Sync>),
}

impl SyncerError {
    pub fn other<E: std::error::Error + Send + Sync + 'static>(err: E) -> Self {
        Self::Other(Box::new(err))
    }
}
