use std::sync::Arc;

use alloy::{primitives::Address, sol_types::SolCall};
use kohaku_kv_store::Store;
use rand::CryptoRng;
use ruint::aliases::U256;
use thiserror::Error;
use tokio::sync::Mutex;

use crate::{
    abis::tornado::Tornado,
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
    syncer: Syncer,
    verifier: Verifier,
    pools: Arc<Mutex<Vec<PoolProvider>>>,
}

#[derive(Debug, Error)]
pub enum TornadoProviderError {
    #[error("Unknown pool: amount={0}, symbol={1}, chain_id={2}")]
    UnknownPool(String, String, u64),
    #[error(transparent)]
    Pool(#[from] PoolProviderError),
}

impl TornadoProvider {
    #[must_use]
    pub fn new(store: Store, syncer: Syncer, verifier: Verifier) -> Self {
        Self {
            store,
            syncer,
            verifier,
            pools: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Get a reference to the provider for a given pool, creating it if it doesn't exist.
    pub async fn pool(&self, pool: Pool) -> PoolProvider {
        let mut pools = self.pools.lock().await;

        if let Some(p) = pools.iter().find(|p| *p.pool() == pool) {
            return p.clone();
        }
        let scoped_store = self.store.scope(pool.id());
        let provider = PoolProvider::new(
            pool,
            scoped_store,
            self.syncer.clone(),
            self.verifier.clone(),
        );

        pools.retain(|p| *p.pool() != pool);
        pools.push(provider.clone());
        provider
    }

    /// Create a deposit transaction and note for a given pool.
    pub async fn deposit(&self, pool: Pool, rng: &mut impl CryptoRng) -> (Call, Note) {
        let provider = self.pool(pool).await;
        provider.deposit(rng)
    }

    /// Create deposit calldata and note for a given pool.
    pub async fn deposit_call(
        &self,
        pool: Pool,
        rng: &mut impl CryptoRng,
    ) -> (Tornado::depositCall, Note) {
        let provider = self.pool(pool).await;
        provider.deposit_call(rng)
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
        let pool = self.pool_from_note(note).await?;

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
        let pool = self.pool_from_note(note).await?;

        let provider = self.pool(pool).await;
        provider.sync().await?;
        Ok(provider
            .withdraw_call(note, recipient, relayer, fee, refund, rng)
            .await?)
    }

    /// Manually trigger a sync of the provider for all pools.
    ///
    /// # Errors
    /// Returns an error if the pool cannot be synced.
    pub async fn sync(&self) -> Result<(), TornadoProviderError> {
        for provider in self.pools.lock().await.iter() {
            provider.sync().await?;
        }
        Ok(())
    }

    /// Get the pool for a given note, preferring an already-registered pool if available.
    pub(crate) async fn pool_from_note(&self, note: &Note) -> Result<Pool, TornadoProviderError> {
        let pools = self.pools.lock().await;

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
