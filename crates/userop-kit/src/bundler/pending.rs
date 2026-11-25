use alloy::{
    providers::RootProvider,
    transports::{TransportError, TransportResult},
};
use web_time::{Duration, Instant};

use crate::{
    bundler::BundlerApi,
    user_operation::{UserOperationHash, UserOperationReceipt},
};

/// A submitted UserOperation awaiting inclusion.
#[derive(Debug, Clone)]
pub struct PendingUserOperation {
    provider: RootProvider,
    hash: UserOperationHash,
    poll_interval: Duration,
    timeout: Duration,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PendingUserOperationError {
    #[error(transparent)]
    Transport(#[from] TransportError),
    #[error("UserOperation {0:?} was not included before the timeout")]
    Timeout(UserOperationHash),
}

impl PendingUserOperation {
    pub fn new(provider: RootProvider, hash: UserOperationHash) -> Self {
        Self {
            provider,
            hash,
            poll_interval: Duration::from_secs(6),
            timeout: Duration::from_secs(60),
        }
    }

    /// The submitted UserOperation's hash.
    pub fn hash(&self) -> UserOperationHash {
        self.hash
    }

    pub fn with_poll_interval(mut self, poll_interval: Duration) -> Self {
        self.poll_interval = poll_interval;
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Polls until the UserOperation is included and returns its receipt.
    ///
    /// # Errors
    /// Returns an error if a receipt request fails or the timeout elapses first.
    pub async fn get_receipt(self) -> Result<UserOperationReceipt, PendingUserOperationError> {
        let start = Instant::now();
        while start.elapsed() < self.timeout {
            if let Some(receipt) = self.poll().await? {
                return Ok(receipt);
            }
            sleep(self.poll_interval).await;
        }

        Err(PendingUserOperationError::Timeout(self.hash))
    }

    async fn poll(&self) -> TransportResult<Option<UserOperationReceipt>> {
        self.provider.get_user_operation_receipt(self.hash).await
    }
}

#[cfg(native)]
async fn sleep(duration: Duration) {
    tokio::time::sleep(duration).await;
}

#[cfg(wasm)]
async fn sleep(duration: Duration) {
    gloo_timers::future::sleep(duration).await;
}
