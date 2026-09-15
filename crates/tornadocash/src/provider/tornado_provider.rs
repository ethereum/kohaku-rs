use std::sync::{Arc, Mutex};

use alloy::{primitives::Address, providers::DynProvider, sol_types::SolCall};
use kohaku_kv_store::Store;
use rand::CryptoRng;
use ruint::aliases::U256;
use thiserror::Error;

use crate::{
    abis::tornado::Tornado,
    circuit::Circuit,
    indexer::{syncer::Syncer, verifier::Verifier},
    note::Note,
    pool::Pool,
    provider::{
        call::Call,
        pool_provider::{PoolProvider, PoolProviderError},
    },
};

/// A provider for multiple tornadocash pools.
///
/// The provider manages multiple `PoolProvider`s for requested pools, providing a unified
/// interface.
#[derive(Clone)]
pub struct TornadoProvider {
    store: Store,
    provider: DynProvider,
    syncer: Syncer,
    verifier: Verifier,
    pools: Arc<Mutex<Vec<PoolProvider>>>,
    circuit: Circuit,
}

#[derive(Debug, Error)]
pub enum TornadoProviderError {
    #[error("Unknown pool: amount={0}, symbol={1}, chain_id={2}")]
    UnknownPool(String, String, u64),
    #[error("Pool not initialized: {0}")]
    PoolNotInitialized(Pool),
    #[error("Lock poisoned")]
    Lock,
    #[error(transparent)]
    Pool(#[from] PoolProviderError),
}

impl TornadoProvider {
    #[must_use]
    pub fn new(
        store: Store,
        provider: DynProvider,
        syncer: Syncer,
        verifier: Verifier,
        circuit: Circuit,
    ) -> Self {
        Self {
            store,
            provider,
            syncer,
            verifier,
            pools: Arc::new(Mutex::new(Vec::new())),
            circuit,
        }
    }

    /// Get a reference to the provider for a given pool, creating it if it doesn't exist.
    pub fn pool(&self, pool: Pool) -> Result<PoolProvider, TornadoProviderError> {
        let mut pools = self.pools.lock().map_err(|_| TornadoProviderError::Lock)?;

        if let Some(p) = pools.iter().find(|p| *p.pool() == pool) {
            return Ok(p.clone());
        }
        let scoped_store = self.store.scope(pool.id());
        let provider = PoolProvider::new(
            pool,
            scoped_store,
            self.provider.clone(),
            self.syncer.clone(),
            self.verifier.clone(),
            self.circuit.clone(),
        );

        pools.retain(|p| *p.pool() != pool);
        pools.push(provider.clone());
        Ok(provider)
    }

    /// Create a deposit transaction and note for a given pool.
    pub fn deposit(
        &self,
        pool: Pool,
        rng: &mut impl CryptoRng,
    ) -> Result<(Call, Note), TornadoProviderError> {
        let provider = self.pool(pool)?;
        Ok(provider.deposit(rng))
    }

    /// Create deposit calldata and note for a given pool.
    pub fn deposit_call(
        &self,
        pool: Pool,
        rng: &mut impl CryptoRng,
    ) -> Result<(Tornado::depositCall, Note), TornadoProviderError> {
        let provider = self.pool(pool)?;
        Ok(provider.deposit_call(rng))
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

        let provider = self.pool(pool)?;
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
        let provider = self.pool(pool)?;
        Ok(provider.quote_wei_in_fee_token(wei_amount).await?)
    }

    /// Manually trigger a sync of the provider for all pools.
    ///
    /// # Errors
    /// Returns an error if the pool cannot be synced.
    pub async fn sync(&self) -> Result<(), TornadoProviderError> {
        for provider in self
            .pools
            .lock()
            .map_err(|_| TornadoProviderError::Lock)?
            .iter()
        {
            provider.sync().await?;
        }
        Ok(())
    }

    /// Get the pool for a given note, preferring an already-registered pool if available.
    pub(crate) fn pool_from_note(&self, note: &Note) -> Result<Pool, TornadoProviderError> {
        let pools = self.pools.lock().map_err(|_| TornadoProviderError::Lock)?;

        if let Some(pool) = pools.iter().map(PoolProvider::pool).find(|pool| {
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
