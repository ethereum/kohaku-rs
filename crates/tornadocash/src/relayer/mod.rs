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

use alloy::primitives::TxHash;
use tracing::warn;

use crate::{
    pool::Pool,
    provider::{TornadoProvider, TornadoProviderError},
    relayer::{
        client::{JobReceipt, JobStatus, RelayerClient, RelayerClientError},
        status::RelayerStatus,
    },
    withdrawal::ProvenWithdrawal,
};

pub mod client;
pub mod status;

/// How often [`Relayer::await_confirmation`] re-checks the relayer and the pool.
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Tornadocash relayer.
///
/// Interacts with a relayer client to create, submit, and monitor withdrawal proofs.
pub struct Relayer {
    client: RelayerClient,
    timeout: Duration,
}

#[derive(Debug, thiserror::Error)]
pub enum RelayerError {
    #[error("Relayer does not support pool: {0}")]
    UnsupportedPool(Pool),
    #[error(transparent)]
    Relayer(#[from] RelayerClientError),
    #[error(transparent)]
    TornadoProvider(#[from] TornadoProviderError),
    #[error("Nullifier still unspent after {timeout:?} (relayer reported: {reason})")]
    NotSpent { timeout: Duration, reason: String },
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
        }
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

    /// Waits until `receipt`'s withdrawal has landed on-chain.
    ///
    /// Returns `Ok(Some(tx_hash))` if the relayer supplied one. Returns `Ok(None)` if the nullifier
    /// was spent but the relayer did not supply a transaction hash.
    ///
    /// # Errors
    /// Returns [`RelayerError::NotSpent`] if the nullifier is still unspent once the timeout
    /// elapses, carrying whatever reason the relayer gave.
    pub async fn await_confirmation(
        &self,
        provider: &TornadoProvider,
        receipt: &JobReceipt,
    ) -> Result<Option<TxHash>, RelayerError> {
        let start = Instant::now();
        let mut tx_hash = None;
        let mut reason = None;

        loop {
            match self.client.job_status(&receipt.id).await {
                Ok(job) => {
                    //? Prefer the most recently reported hash; relayers may resubmit.
                    tx_hash = job.tx_hash.or(tx_hash);
                    if job.status == JobStatus::Failed {
                        reason = Some(job.failed_reason.unwrap_or_else(|| "failed".to_string()));
                    }
                }
                //? The chain is authoritative, so an unreachable relayer is not fatal here.
                Err(e) => {
                    warn!("Failed to poll relayer job status: {}", e);
                    reason = Some(e.to_string());
                }
            }

            if provider
                .is_spent(&receipt.pool, receipt.nullifier_hash)
                .await?
            {
                return Ok(tx_hash);
            }

            if start.elapsed() >= self.timeout {
                return Err(RelayerError::NotSpent {
                    timeout: self.timeout,
                    reason: reason.unwrap_or_else(|| "no error".to_string()),
                });
            }

            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }
}
