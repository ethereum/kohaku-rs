///! Tornadocash Relayers
///!
///! Tornadocash relayers are services run by third parties that accept and submit withdrawal
///! proofs on behalf of users. They allow users to withdraw from Tornadocash to a fresh
///! address that has no ETH to pay for gas at the cost of a small fee. Relayers are run by
///! independent parties, and the Tornadocash DAO does not endorse any particular relayers.
///!
///! See [tornado-relayer](https://github.com/tornado-dao/tornado-relayer/tree/mainnet-v5) for
///! the reference implementation.
use alloy::{
    network::TransactionBuilder,
    primitives::{Address, B256, TxHash},
    providers::{DynProvider, Provider},
    rpc::types::TransactionRequest,
    sol_types::SolCall,
};
use rand::CryptoRng;
use ruint::aliases::U256;

use crate::{
    abis::tornado::Tornado,
    note::Note,
    pool::Pool,
    provider::tornado_provider::{TornadoProvider, TornadoProviderError},
    relayer::client::{JobReceipt, JobStatus, RelayerClient, RelayerClientError, RelayerStatus},
};

pub mod client;

/// Tornadocash relayer provider.
///
/// Interacts with a Tornadocash relayer to create, submit, and monitor withdrawal proofs.
pub struct RelayerProvider {
    relayer: RelayerClient,
    tornado: TornadoProvider,
    provider: DynProvider,
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

impl RelayerProvider {
    #[must_use]
    pub fn new(relayer: RelayerClient, tornado: TornadoProvider, provider: DynProvider) -> Self {
        Self {
            relayer,
            tornado,
            provider,
            timeout: std::time::Duration::from_secs(30),
        }
    }

    /// Returns a reference to the underlying Tornadocash provider.
    pub fn tornado(&self) -> &TornadoProvider {
        &self.tornado
    }

    /// Quotes the relayer's current fee for withdrawing `note`, given `refund`.
    ///
    /// # Errors
    /// Returns an error if the pool cannot be resolved, the relayer cannot be reached, or the
    /// relayer does not support the note's pool.
    pub async fn estimate_fee(
        &self,
        note: &Note,
        refund: U256,
    ) -> Result<U256, RelayerProviderError> {
        let pool = self.tornado.pool_from_note(note).await?;
        let (_, fee) = self.quote(&pool, refund).await?;
        Ok(fee)
    }

    /// Builds a withdrawal proof for `note` and submits it to the relayer.
    ///
    /// # Errors
    /// Returns an error if the pool cannot be resolved, the relayer does not support it, the
    /// withdrawal proof cannot be built, or the relayer rejects the submission.
    pub async fn withdraw(
        &mut self,
        note: &Note,
        recipient: Address,
        refund: U256,
        rng: &mut impl CryptoRng,
    ) -> Result<JobReceipt, RelayerProviderError> {
        let pool = self.tornado.pool_from_note(note).await?;
        let (status, fee) = self.quote(&pool, refund).await?;

        let call = self
            .tornado
            .withdraw_call(
                note,
                recipient,
                Some(status.reward_account),
                Some(fee),
                Some(refund),
                rng,
            )
            .await?;

        Ok(self.relayer.withdraw(&pool, call).await?)
    }

    /// Polls the relayer until `receipt`'s withdrawal is confirmed on-chain.
    ///
    /// # Errors
    /// Returns an error if the relayer cannot be reached or the job times out.
    pub async fn await_confirmation(
        &self,
        receipt: &JobReceipt,
    ) -> Result<Option<TxHash>, RelayerProviderError> {
        let start = std::time::Instant::now();
        while start.elapsed() < self.timeout {
            if let PollOutcome::Done(tx_hash) = self.poll_job(receipt, &self.provider).await? {
                return Ok(tx_hash);
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }

        if is_nullifier_spent(&self.provider, receipt.pool.address, receipt.nullifier_hash).await? {
            return Ok(None);
        }

        Err(RelayerProviderError::Timeout)
    }

    /// Fetches the relayer's status and quotes a fee for `pool`.
    async fn quote(
        &self,
        pool: &Pool,
        refund: U256,
    ) -> Result<(RelayerStatus, U256), RelayerProviderError> {
        let status = self.relayer.status().await?;
        let gas_price = self.provider.get_gas_price().await?;
        let fee = status
            .fee(pool, gas_price, U256::from(pool.amount_wei), refund)
            .ok_or(RelayerProviderError::UnsupportedPool(*pool))?;

        Ok((status, fee))
    }

    /// Polls the relayer for the status of `receipt`'s withdrawal job.
    ///
    /// Follows the following logic:
    /// - If the job is failed, check and return whether the nullifier has been spent.
    /// - If the job is confirmed, check and return whether the transaction has been mined.
    /// - If the job is pending, return `PollOutcome::Pending`.
    ///
    /// NOTE: It is possible for a relayer to hold onto a job indefinitely, so a failed
    /// job does not guarantee that the withdrawal will not later be mined. Once a
    /// withdrawal proof has been shared, assume it might be mined at any time.
    async fn poll_job(
        &self,
        receipt: &JobReceipt,
        provider: &DynProvider,
    ) -> Result<PollOutcome, RelayerProviderError> {
        let job = self.relayer.job_status(receipt).await?;

        if job.status == JobStatus::Failed {
            if is_nullifier_spent(provider, receipt.pool.address, receipt.nullifier_hash).await? {
                return Ok(PollOutcome::Done(None));
            }
            let reason = job
                .failed_reason
                .unwrap_or_else(|| "unknown error".to_string());
            return Err(RelayerProviderError::Relayer(
                RelayerClientError::RelayerError(reason),
            ));
        }

        if job.status != JobStatus::Confirmed {
            return Ok(PollOutcome::Pending);
        }
        let Some(tx_hash) = job.tx_hash else {
            return Ok(PollOutcome::Pending);
        };
        if provider.get_transaction_receipt(tx_hash).await?.is_none() {
            return Ok(PollOutcome::Pending);
        }

        Ok(PollOutcome::Done(Some(tx_hash)))
    }
}

async fn is_nullifier_spent(
    provider: &DynProvider,
    pool_address: Address,
    nullifier_hash: B256,
) -> Result<bool, RelayerProviderError> {
    let call = Tornado::isSpentCall::new((nullifier_hash,)).abi_encode();

    let result = provider
        .call(
            TransactionRequest::default()
                .with_to(pool_address)
                .input(call.into()),
        )
        .await?;

    Ok(Tornado::isSpentCall::abi_decode_returns(&result)?)
}
