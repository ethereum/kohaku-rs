use alloy::{primitives::Address, providers::DynProvider, sol_types::SolCall};
use kohaku_kv_store::Store;
use rand::CryptoRng;
use ruint::aliases::U256;
use thiserror::Error;

use crate::{
    abis::tornado::Tornado,
    circuit::Circuit,
    indexer::{syncer::Syncer, verifier::Verifier},
    provider::{
        call::Call,
        note::Note,
        pool::Pool,
        pool_provider::{PoolProvider, PoolProviderError},
    },
};

/// A provider for multiple tornadocash pools.
///
/// The provider manages multiple `PoolProvider`s for requested pools, providing a unified
/// interface.
pub struct TornadoProvider {
    provider: DynProvider,
    store: Store,
    syncer: Syncer,
    verifier: Verifier,
    pools: Vec<PoolProvider>,
    circuit: Circuit,
}

#[derive(Debug, Error)]
pub enum TornadoProviderError {
    #[error("Unknown pool: amount={0}, symbol={1}, chain_id={2}")]
    UnknownPool(String, String, u64),
    #[error("Pool not initialized: {0}")]
    PoolNotInitialized(Pool),
    #[error(transparent)]
    Pool(#[from] PoolProviderError),
}

impl TornadoProvider {
    #[must_use]
    pub fn new(
        provider: DynProvider,
        store: Store,
        syncer: Syncer,
        verifier: Verifier,
        circuit: Circuit,
    ) -> Self {
        Self {
            provider,
            store,
            syncer,
            verifier,
            pools: Vec::new(),
            circuit,
        }
    }

    /// Get a mutable reference to the provider for a given pool, creating it if it doesn't exist.
    pub fn pool(&mut self, pool: Pool) -> &mut PoolProvider {
        if let Some(i) = self.pools.iter().position(|p| *p.pool() == pool) {
            return &mut self.pools[i];
        }

        let scoped_store = self.store.scope(pool.id());
        let provider = PoolProvider::new(
            pool,
            self.provider.clone(),
            scoped_store,
            self.syncer.clone(),
            self.verifier.clone(),
            self.circuit.clone(),
        );

        self.pools.retain(|p| *p.pool() != pool);
        self.pools.push(provider);

        #[expect(
            clippy::missing_panics_doc,
            reason = "We just pushed a new provider, so the last element is guaranteed to be available."
        )]
        self.pools.last_mut().unwrap()
    }

    /// Create a deposit transaction and note for a given pool.
    ///
    /// The pool must already be initialized (e.g. via [`TornadoProvider::pool`]); otherwise
    /// returns [`TornadoProviderError::PoolNotInitialized`].
    ///
    /// # Errors
    /// Returns an error if the pool is not initialized.
    pub fn deposit(
        &self,
        pool: Pool,
        rng: &mut impl CryptoRng,
    ) -> Result<(Call, Note), TornadoProviderError> {
        let provider = self
            .pools
            .iter()
            .find(|p| *p.pool() == pool)
            .ok_or(TornadoProviderError::PoolNotInitialized(pool))?;
        Ok(provider.deposit(rng))
    }

    /// Create a withdrawal transaction for the given note to the recipient address.
    ///
    /// # Errors
    /// Returns an error if the pool cannot be found, is not initialized, or if the withdrawal
    /// cannot be created.
    pub async fn withdraw(
        &mut self,
        note: &Note,
        recipient: Address,
        relayer: Option<Address>,
        fee: Option<U256>,
        refund: Option<U256>,
        rng: &mut impl CryptoRng,
    ) -> Result<Call, TornadoProviderError> {
        let pool = self.pool_from_note(note)?;

        let data = self
            .withdraw_call(note, recipient, relayer, fee, refund, rng)
            .await?
            .abi_encode();

        Ok(Call::new(
            pool.address,
            data.into(),
            refund.unwrap_or_default(),
        ))
    }

    /// Create withdrawal calldata.
    ///
    /// # Errors
    /// Returns an error if the pool cannot be synced or the withdrawal call cannot be created.
    pub async fn withdraw_call(
        &mut self,
        note: &Note,
        recipient: Address,
        relayer: Option<Address>,
        fee: Option<U256>,
        refund: Option<U256>,
        rng: &mut impl CryptoRng,
    ) -> Result<Tornado::withdrawCall, TornadoProviderError> {
        let pool = self.pool_from_note(note)?;

        let provider = self.pool(pool);
        provider.sync().await?;
        Ok(provider
            .withdraw_call(note, recipient, relayer, fee, refund, rng)
            .await?)
    }

    /// Quote the amount of fee token from a given wei amount. If the pool is native, this is a
    /// no-op.
    ///
    /// # Errors
    /// Returns an error if the quote cannot be queried.
    pub async fn quote_wei_in_fee_token(
        &mut self,
        pool: Pool,
        wei_amount: U256,
    ) -> Result<U256, TornadoProviderError> {
        let provider = self.pool(pool);
        Ok(provider.quote_wei_in_fee_token(wei_amount).await?)
    }

    /// Manually trigger a sync of the provider for all pools.
    ///
    /// # Errors
    /// Returns an error if the pool cannot be synced.
    pub async fn sync(&mut self) -> Result<(), TornadoProviderError> {
        for provider in &mut self.pools {
            provider.sync().await?;
        }
        Ok(())
    }

    /// Manually trigger a sync of the provider for all pools up to the given block.
    ///
    /// # Errors
    /// Returns an error if the pool cannot be synced.
    pub async fn sync_to(&mut self, block: u64) -> Result<(), TornadoProviderError> {
        for provider in &mut self.pools {
            provider.sync_to(block).await?;
        }
        Ok(())
    }

    /// Get the pool for a given note, preferring an already-registered pool if available.
    pub(crate) fn pool_from_note(&self, note: &Note) -> Result<Pool, TornadoProviderError> {
        if let Some(pool) = self.pools.iter().map(PoolProvider::pool).find(|pool| {
            pool.chain_id == note.chain_id
                && pool.symbol() == note.symbol
                && pool.amount() == note.amount
        }) {
            return Ok(*pool);
        }

        Pool::from_raw(&note.amount, &note.symbol, note.chain_id).ok_or_else(|| {
            TornadoProviderError::UnknownPool(
                note.amount.clone(),
                note.symbol.clone(),
                note.chain_id,
            )
        })
    }
}
