pub(crate) mod pool_provider;

use std::sync::Arc;

use alloy::{
    primitives::{Address, B256},
    providers::DynProvider,
};
use kohaku_kv_store::Store;
use rand::{CryptoRng, RngExt};
use ruint::aliases::U256;
use thiserror::Error;
use tokio::sync::Mutex;

use crate::{
    deposit::Deposit,
    indexer::{Indexer, syncer::Syncer, verifier::Verifier},
    note::Note,
    pool::Pool,
    provider::pool_provider::{PoolProvider, PoolProviderError},
    withdrawal::Withdrawal,
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
    provider: DynProvider,

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
    pub fn new(store: Store, syncer: Syncer, verifier: Verifier, provider: DynProvider) -> Self {
        Self {
            store,
            syncer,
            verifier,
            provider,
            pools: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Returns the underlying alloy DynProvider
    pub(crate) fn inner_provider(&self) -> DynProvider {
        self.provider.clone()
    }

    /// Sync all pools managed by this provider.
    ///
    /// # Errors
    /// Returns an error if the pool cannot be synced.
    pub async fn sync(&self) -> Result<(), TornadoProviderError> {
        for provider in self.pools.lock().await.iter() {
            provider.sync().await?;
        }
        Ok(())
    }

    /// Create a deposit for the given pool.
    pub fn deposit(&self, pool: Pool, rng: &mut impl CryptoRng) -> Deposit {
        Deposit::new(pool, rng.random(), rng.random())
    }

    /// Create a withdrawal for the given note.
    ///
    /// # Errors
    /// Returns an error if the note's pool cannot be found.
    pub async fn withdraw(
        &self,
        note: Note,
        recipient: Address,
    ) -> Result<Withdrawal, TornadoProviderError> {
        let pool = self.pool_from_note(&note).await?;
        Ok(Withdrawal::new(self.clone(), pool, note, recipient))
    }

    /// Quote the amount of fee token from a given wei amount.
    ///
    /// # Errors
    /// Returns an error if a contract call fails.
    pub async fn quote_wei_in_fee_token(
        &self,
        pool: Pool,
        wei_amount: U256,
    ) -> Result<U256, TornadoProviderError> {
        let provider = self.provider(pool).await;
        Ok(provider.quote_wei_in_fee_token(wei_amount).await?)
    }

    /// Check if a note has been spent.
    ///
    /// # Errors
    /// Returns an error if the pool for this note cannot be found or if a contract call fails.
    pub async fn is_spent(&self, note: &Note) -> Result<bool, TornadoProviderError> {
        let provider = self.provider_from_note(note).await?;
        Ok(provider.is_spent(note.nullifier_hash().into()).await?)
    }

    /// Checks if a nullifier hash has been spent in a given pool.
    ///
    /// # Errors
    /// Returns an error if the pool cannot be found or if a contract call fails.
    pub async fn is_nullifier_spent(
        &self,
        pool: Pool,
        nullifier_hash: B256,
    ) -> Result<bool, TornadoProviderError> {
        let provider = self.provider(pool).await;
        Ok(provider.is_spent(nullifier_hash).await?)
    }

    /// Gets the pool provider for a given note, creating it if it doesn't exist.
    ///
    /// # Errors
    /// Returns an error if the pool cannot be created.
    pub(crate) async fn provider_from_note(
        &self,
        note: &Note,
    ) -> Result<PoolProvider, TornadoProviderError> {
        let pool = self.pool_from_note(note).await?;
        Ok(self.provider(pool).await)
    }

    /// Get the pool for a given note.
    ///
    /// Searches both this provider's registered pools and the set of known pools for a pool
    /// matching the note's parameters.
    pub(crate) async fn pool_from_note(&self, note: &Note) -> Result<Pool, TornadoProviderError> {
        let pools = self.pools.lock().await;

        if let Some(pool) = pools.iter().map(PoolProvider::pool).find(|pool| {
            pool.chain_id == note.chain_id
                && pool.symbol() == note.symbol
                && pool.amount() == note.amount
        }) {
            return Ok(pool);
        }

        Pool::from_raw(&note.amount, &note.symbol, note.chain_id).ok_or_else(|| {
            TornadoProviderError::UnknownPool(
                note.amount.clone(),
                note.symbol.clone(),
                note.chain_id,
            )
        })
    }

    /// Get a reference to the provider for a given pool, creating it if it doesn't exist.
    async fn provider(&self, pool: Pool) -> PoolProvider {
        let mut pools = self.pools.lock().await;

        if let Some(p) = pools.iter().find(|p| p.pool() == pool) {
            return p.clone();
        }

        let indexer = Indexer::new(
            pool,
            self.store.scope(pool.id()),
            self.syncer.clone(),
            self.verifier.clone(),
        );
        let provider = PoolProvider::new(indexer, self.provider.clone());

        pools.push(provider.clone());
        provider
    }
}
