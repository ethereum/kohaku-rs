use std::fmt;

use alloy::{
    network::TransactionBuilder, primitives::Address, providers::Provider,
    rpc::types::TransactionRequest, sol_types::SolCall,
};
use rand::CryptoRng;
use ruint::aliases::U256;

use crate::{
    abis::tornado::Tornado::withdrawCall,
    note::Note,
    pool::Pool,
    provider::{TornadoProvider, TornadoProviderError},
    relayer::{Relayer, RelayerError, client::JobReceipt},
};

#[derive(Clone)]
pub struct Withdrawal {
    provider: TornadoProvider,
    note: Note,
    recipient: Address,
    relayer: Option<Address>,
    fee: Option<U256>,
    refund: Option<U256>,
}

impl Withdrawal {
    pub fn new(provider: TornadoProvider, note: Note, recipient: Address) -> Self {
        Self {
            provider,
            note,
            recipient,
            relayer: None,
            fee: None,
            refund: None,
        }
    }

    pub fn with_refund(mut self, refund: U256) -> Self {
        self.refund = Some(refund);
        self
    }

    pub fn with_relayer(mut self, relayer: Address) -> Self {
        self.relayer = Some(relayer);
        self
    }

    pub fn with_fee(mut self, fee: U256) -> Self {
        self.fee = Some(fee);
        self
    }

    /// Returns the pool this withdrawal is for.
    #[must_use]
    pub async fn pool(&self) -> Result<Pool, TornadoProviderError> {
        self.provider.pool_from_note(&self.note).await
    }

    /// Returns the provider this withdrawal was created from.
    #[must_use]
    pub fn provider(&self) -> &TornadoProvider {
        &self.provider
    }

    /// Converts the withdrawal into an alloy [`TransactionRequest`].
    pub async fn into_transaction(
        self,
        rng: &mut impl CryptoRng,
    ) -> Result<TransactionRequest, TornadoProviderError> {
        let call = self.as_call(rng).await?;

        Ok(TransactionRequest::default()
            .with_to(self.pool().await?.address)
            .with_input(call.abi_encode())
            .with_value(self.refund.unwrap_or_default()))
    }

    /// Submits the withdrawal to this relayer.
    ///
    /// Overrides any already set relayer or fee values.
    pub async fn relay(
        mut self,
        relayer: &Relayer,
        rng: &mut impl CryptoRng,
    ) -> Result<JobReceipt, RelayerError> {
        let status = relayer.status().await?;
        let gas_price = self.provider.inner_provider().get_gas_price().await?;

        self.fee = Some(status.fee(
            &self.pool().await?,
            gas_price,
            self.refund.unwrap_or_default(),
        )?);
        self.relayer = Some(status.reward_account);

        let call = self.as_call(rng).await?;
        relayer.withdraw(&self.pool().await?, call).await
    }

    /// Converts the withdrawal into a raw [`withdrawCall`] struct.
    pub(crate) async fn into_call(
        self,
        rng: &mut impl CryptoRng,
    ) -> Result<withdrawCall, TornadoProviderError> {
        self.as_call(rng).await
    }

    async fn as_call(
        &self,
        rng: &mut impl CryptoRng,
    ) -> Result<withdrawCall, TornadoProviderError> {
        Ok(self
            .provider
            .provider_from_note(&self.note)
            .await?
            .prove_withdrawal(
                &self.note,
                self.recipient,
                self.relayer,
                self.fee,
                self.refund,
                rng,
            )
            .await?)
    }
}

impl fmt::Debug for Withdrawal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Withdrawal")
            .field("note", &self.note)
            .field("recipient", &self.recipient)
            .field("relayer", &self.relayer)
            .field("fee", &self.fee)
            .field("refund", &self.refund)
            .finish()
    }
}
