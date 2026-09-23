use std::{ops::Range, sync::Arc};

use ruint::aliases::U256;
use thiserror::Error;

use crate::pool::Pool;

#[derive(Debug, Clone)]
pub enum SyncEvent {
    LeafAppended {
        cm: U256,
        epoch: u64,
        index: u32,
        new_root: U256,
    },
    EpochRolled {
        closed: u64,
        new_epoch: u64,
    },
    NoteSpent {
        nf: U256,
    },
    RootPublished {
        epoch: u64,
    },
}

#[async_trait::async_trait]
pub trait SyncerBackend: Send + Sync {
    async fn latest_block(&self, pool: &Pool) -> Result<u64, SyncerError>;
    async fn sync(&self, pool: &Pool, from: u64, to: u64) -> Result<Vec<SyncEvent>, SyncerError>;
}

#[derive(Clone)]
pub struct Syncer(Arc<dyn SyncerBackend>);

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

    pub async fn latest_block(&self, pool: &Pool) -> Result<u64, SyncerError> {
        self.0.latest_block(pool).await
    }

    pub async fn sync(
        &self,
        pool: &Pool,
        range: Range<u64>,
    ) -> Result<Vec<SyncEvent>, SyncerError> {
        self.0.sync(pool, range.start, range.end).await
    }
}

impl SyncerError {
    pub fn other<E: std::error::Error + Send + Sync + 'static>(err: E) -> Self {
        Self::Other(Box::new(err))
    }
}
