//! Tornadocash Relayers
//!
//! Tornadocash relayers are services run by third parties that accept and submit withdrawal
//! proofs on behalf of users. They allow users to withdraw from Tornadocash to a fresh
//! address that has no ETH to pay for gas at the cost of a small fee. Relayers are run by
//! independent parties, and the Tornadocash DAO does not endorse any particular relayers.
//!
//! See [tornado-relayer](https://github.com/tornado-dao/tornado-relayer/tree/mainnet-v5) for
//! the reference implementation.
use alloy::{
    primitives::{Address, TxHash},
    providers::Provider,
};
use rand::CryptoRng;
use ruint::aliases::U256;

use crate::{
    note::Note,
    pool::Pool,
    provider::{TornadoProvider, TornadoProviderError},
    relayer::{
        client::{JobReceipt, JobResponse, JobStatus, RelayerClient, RelayerClientError},
        status::RelayerStatus,
    },
};

pub mod client;
pub mod status;

/// Tornadocash relayer.
///
/// Interacts with a relayer client to create, submit, and monitor withdrawal proofs.
pub struct Relayer {
    client: RelayerClient,
    timeout: std::time::Duration,
}

#[derive(Debug, thiserror::Error)]
pub enum RelayerProviderError {
    #[error("Relayer does not support pool: {0}")]
    UnsupportedPool(Pool),
    #[error(transparent)]
    Relayer(#[from] RelayerClientError),
    #[error(transparent)]
    TornadoProvider(#[from] TornadoProviderError),
    #[error("Sol error: {0}")]
    Sol(#[from] alloy::sol_types::Error),
    #[error("Provider error: {0}")]
    Provider(#[from] alloy::transports::RpcError<alloy::transports::TransportErrorKind>),
    #[error("Timed out waiting for job confirmation")]
    Timeout,
}

enum PollOutcome {
    Pending,
    Done(Option<TxHash>),
}

/// What to do next given the relayer's reported job status. Kept free of I/O so the decision
/// logic can be unit tested without a live relayer or provider.
#[derive(Debug, PartialEq, Eq)]
enum JobAction {
    Pending,
    CheckNullifierSpent { failed_reason: String },
    CheckReceipt(TxHash),
}

impl Relayer {
    #[must_use]
    pub fn new(url: &str) -> Self {
        let client = RelayerClient::new(url);
        Self {
            client,
            timeout: std::time::Duration::from_secs(30),
        }
    }

    #[must_use]
    pub fn from_client(client: RelayerClient) -> Self {
        Self {
            client,
            timeout: std::time::Duration::from_secs(30),
        }
    }

    /// Quotes the relayer's current fee for withdrawing `note`.
    ///
    /// # Errors
    /// Returns an error if the pool cannot be found, the relayer cannot be reached, or does not
    /// support the pool.
    pub async fn estimate_fee(
        &self,
        provider: &TornadoProvider,
        note: &Note,
        refund: Option<U256>,
    ) -> Result<U256, RelayerProviderError> {
        let pool = provider.pool_from_note(note).await?;
        let (_, fee) = self.quote(provider, &pool, refund).await?;
        Ok(fee)
    }

    /// Builds a withdrawal proof for `note` and submits it to the relayer.
    ///
    /// # Errors
    /// Returns an error if the pool cannot be resolved, the relayer does not support it, the
    /// withdrawal proof cannot be built, or the relayer rejects the submission.
    pub async fn withdraw(
        &self,
        provider: &TornadoProvider,
        note: &Note,
        recipient: Address,
        refund: Option<U256>,
        rng: &mut impl CryptoRng,
    ) -> Result<JobReceipt, RelayerProviderError> {
        let pool = provider.pool_from_note(note).await?;
        let (status, fee) = self.quote(provider, &pool, refund).await?;

        let call = provider
            .withdraw_call(
                note,
                recipient,
                Some(status.reward_account),
                Some(fee),
                refund,
                rng,
            )
            .await?;

        Ok(self.client.withdraw(&pool, call).await?)
    }

    /// Polls the relayer until `receipt`'s withdrawal is confirmed on-chain.
    ///
    /// # Errors
    /// Returns an error if the relayer cannot be reached or the job times out.
    pub async fn await_confirmation(
        &self,
        provider: &TornadoProvider,
        receipt: &JobReceipt,
    ) -> Result<Option<TxHash>, RelayerProviderError> {
        let start = std::time::Instant::now();
        while start.elapsed() < self.timeout {
            if let PollOutcome::Done(tx_hash) = self.poll_job(provider, receipt).await? {
                return Ok(tx_hash);
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }

        if provider
            .is_nullifier_spent(receipt.pool, receipt.nullifier_hash)
            .await?
        {
            return Ok(None);
        }

        Err(RelayerProviderError::Timeout)
    }

    /// Fetches the relayer's status and quotes a fee for `pool`.
    async fn quote(
        &self,
        provider: &TornadoProvider,
        pool: &Pool,
        refund: Option<U256>,
    ) -> Result<(RelayerStatus, U256), RelayerProviderError> {
        let gas_price = provider.inner_provider().get_gas_price().await?;
        let amount = U256::from(pool.amount_wei);
        let refund = refund.unwrap_or_default();

        let status = self.client.status().await?;
        let fee = status
            .fee(pool, gas_price, amount, refund)
            .ok_or(RelayerProviderError::UnsupportedPool(*pool))?;

        Ok((status, fee))
    }

    /// Polls the relayer for the status of `receipt`'s withdrawal job.
    async fn poll_job(
        &self,
        provider: &TornadoProvider,
        receipt: &JobReceipt,
    ) -> Result<PollOutcome, RelayerProviderError> {
        let job = self.client.job_status(&receipt.id).await?;

        match decide_job_action(&job) {
            JobAction::Pending => Ok(PollOutcome::Pending),
            JobAction::CheckNullifierSpent { failed_reason } => {
                let is_spent = provider
                    .is_nullifier_spent(receipt.pool, receipt.nullifier_hash)
                    .await?;

                match is_spent {
                    true => Ok(PollOutcome::Done(None)),
                    false => Err(RelayerProviderError::Relayer(
                        RelayerClientError::RelayerError(failed_reason),
                    )),
                }
            }
            JobAction::CheckReceipt(tx_hash) => {
                let receipt = provider
                    .inner_provider()
                    .get_transaction_receipt(tx_hash)
                    .await?;

                match receipt {
                    Some(_) => Ok(PollOutcome::Done(Some(tx_hash))),
                    None => Ok(PollOutcome::Pending),
                }
            }
        }
    }
}

/// Helper to decide the next action based on the relayer's reported status. Sans-I/O
/// for simpler unit testing.
fn decide_job_action(job: &JobResponse) -> JobAction {
    if job.status == JobStatus::Failed {
        return JobAction::CheckNullifierSpent {
            failed_reason: job
                .failed_reason
                .clone()
                .unwrap_or_else(|| "unknown error".to_string()),
        };
    }

    if job.status != JobStatus::Confirmed {
        return JobAction::Pending;
    }

    match job.tx_hash {
        Some(tx_hash) => JobAction::CheckReceipt(tx_hash),
        None => JobAction::Pending,
    }
}

#[cfg(test)]
mod tests {
    use alloy::primitives::b256;

    use super::*;

    fn job(status: JobStatus, tx_hash: Option<TxHash>, failed_reason: Option<&str>) -> JobResponse {
        JobResponse {
            status,
            tx_hash,
            confirmations: None,
            failed_reason: failed_reason.map(str::to_string),
        }
    }

    #[test]
    fn pending_while_unconfirmed() {
        for status in [
            JobStatus::Queued,
            JobStatus::Accepted,
            JobStatus::Sent,
            JobStatus::Mined,
            JobStatus::Resubmitted,
        ] {
            assert_eq!(
                decide_job_action(&job(status, None, None)),
                JobAction::Pending
            );
        }
    }

    #[test]
    fn pending_when_confirmed_without_tx_hash() {
        assert_eq!(
            decide_job_action(&job(JobStatus::Confirmed, None, None)),
            JobAction::Pending
        );
    }

    #[test]
    fn checks_receipt_when_confirmed_with_tx_hash() {
        let tx_hash = b256!("0x0000000000000000000000000000000000000000000000000000000000000001");
        assert_eq!(
            decide_job_action(&job(JobStatus::Confirmed, Some(tx_hash), None)),
            JobAction::CheckReceipt(tx_hash)
        );
    }

    #[test]
    fn checks_nullifier_when_failed() {
        assert_eq!(
            decide_job_action(&job(JobStatus::Failed, None, Some("boom"))),
            JobAction::CheckNullifierSpent {
                failed_reason: "boom".to_string()
            }
        );
    }

    #[test]
    fn failed_without_reason_defaults_to_unknown_error() {
        assert_eq!(
            decide_job_action(&job(JobStatus::Failed, None, None)),
            JobAction::CheckNullifierSpent {
                failed_reason: "unknown error".to_string()
            }
        );
    }
}
