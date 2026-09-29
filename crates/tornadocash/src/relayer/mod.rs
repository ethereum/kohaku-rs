//! Tornadocash Relayer Client
//!
//! Tornadocash relayers are services run by third parties that accept and submit withdrawal
//! proofs on behalf of users. They allow users to withdraw from Tornadocash to a fresh
//! address that has no ETH to pay for gas at the cost of a small fee. Relayers are run by
//! independent parties, and the Tornadocash DAO does not endorse any particular relayers.
//!
//! See [tornado-relayer](https://github.com/tornado-dao/tornado-relayer/tree/mainnet-v5) for
//! the reference implementation.
use std::time::{Duration, Instant};

use alloy::{primitives::TxHash, providers::Provider};
use serde::{Deserialize, Serialize};

use crate::{
    pool::Pool,
    provider::TornadoProviderExt,
    relayer::{
        client::{JobReceipt, JobStatus, RelayerClient, RelayerClientError},
        status::RelayerStatus,
    },
    withdrawal::ProvenWithdrawal,
};

pub mod client;
pub mod status;

/// Tornadocash relayer.
///
/// Interacts with a relayer client to create, submit, and monitor withdrawal proofs.
pub struct Relayer {
    client: RelayerClient,
    timeout: Duration,
    poll_interval: Duration,
}

/// The status of a relayed withdrawal.
///
/// `spent` should be considered authoritative, because it's queried directly
/// from the chain. `tx_hash` and `job` are reported by the relayer which may be
/// unreliable or malicious.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WithdrawalStatus {
    /// Whether the nullifier has been spent on-chain.
    pub spent: bool,
    /// The reported transaction hash, if any.
    pub tx_hash: Option<TxHash>,
    /// The reported job status, if any.
    pub job: Option<JobStatus>,
}

#[derive(Debug, thiserror::Error)]
pub enum RelayerError {
    #[error("Relayer does not support pool: {0}")]
    UnsupportedPool(Pool),
    #[error("Relayer client error: {0}")]
    Client(#[from] RelayerClientError),
    #[error("Provider error: {0}")]
    Provider(#[from] alloy::contract::Error),
    #[error("Relayer error: {reason}")]
    JobFailed {
        reason: String,
        tx_hash: Option<TxHash>,
    },
    #[error("Timeout after {timeout:?} (last status: ({last_status:?}): {reason})")]
    Timeout {
        timeout: Duration,
        last_status: Option<JobStatus>,
        reason: String,
    },
}

impl Relayer {
    #[must_use]
    pub fn new(url: &str) -> Self {
        Self::from_client(RelayerClient::new(url))
    }

    #[must_use]
    pub fn from_client(client: RelayerClient) -> Self {
        Self {
            client,
            timeout: Duration::from_secs(30),
            poll_interval: Duration::from_millis(500),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_poll_interval(mut self, poll_interval: Duration) -> Self {
        self.poll_interval = poll_interval;
        self
    }

    /// Retrieves the relayer's status.
    ///
    /// # Errors
    /// Returns an error if the relayer cannot be reached.
    pub async fn status(&self) -> Result<RelayerStatus, RelayerError> {
        Ok(self.client.status().await?)
    }

    /// Submits a withdrawal request to the relayer.
    ///
    /// # Errors
    /// Returns an error if the request cannot be submitted or the relayer returns an error.
    pub async fn withdraw(&self, withdrawal: ProvenWithdrawal) -> Result<JobReceipt, RelayerError> {
        let receipt = self.client.withdraw(withdrawal).await?;
        Ok(receipt)
    }

    /// Checks the status of a relayed withdrawal.
    ///
    /// # Errors
    /// Returns an error if the relayer cannot be reached, the relayer returns an error, or the
    /// provider returns an error.
    pub async fn check(
        &self,
        provider: &impl Provider,
        receipt: &JobReceipt,
    ) -> Result<WithdrawalStatus, RelayerError> {
        let (job, spent) = tokio::join!(
            self.client.job_status(&receipt.id),
            provider.is_spent(&receipt.pool, receipt.nullifier_hash)
        );
        let spent = spent?;

        //? If the nullifier is spent, we don't care about the job status so early-exit.
        if spent {
            let job = job.ok();
            return Ok(WithdrawalStatus {
                spent: true,
                tx_hash: job.as_ref().and_then(|j| j.tx_hash),
                job: job.as_ref().map(|j| j.status),
            });
        }

        let job = job?;
        if job.status == JobStatus::Failed {
            return Err(RelayerError::JobFailed {
                reason: job.failed_reason.unwrap_or_else(|| "failed".to_string()),
                tx_hash: job.tx_hash,
            });
        }

        Ok(WithdrawalStatus {
            spent: false,
            tx_hash: job.tx_hash,
            job: Some(job.status),
        })
    }

    /// Polls until `receipt`'s withdrawal has landed on-chain.
    ///
    /// Returns the transaction hash or `None` if one was not supplied by the relayer.
    #[cfg(native)]
    pub async fn await_confirmation(
        &self,
        provider: &impl Provider,
        receipt: &JobReceipt,
    ) -> Result<Option<TxHash>, RelayerError> {
        let start = Instant::now();
        let mut last_status;

        loop {
            let status = self.check(provider, receipt).await?;
            if status.spent {
                return Ok(status.tx_hash);
            }

            last_status = status.job;

            if start.elapsed() > self.timeout {
                return Err(RelayerError::Timeout {
                    timeout: self.timeout,
                    last_status,
                    reason: "withdrawal did not land on-chain in time".to_string(),
                });
            }

            tokio::time::sleep(self.poll_interval).await;
        }
    }
}
