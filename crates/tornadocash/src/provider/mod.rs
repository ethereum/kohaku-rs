mod known_pools;
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
use tokio::sync::{Mutex, OnceCell};

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
/// interface. Pools are tracked automatically: any pool ever passed to `deposit`, `withdraw`, or
/// the other pool-scoped methods is remembered (persisted to `store`) so that a fresh
/// `TornadoProvider` over the same store picks it back up without the caller needing to
/// re-register it, and so `sync` finds it without being told which pools to look at.
#[derive(Clone)]
pub struct TornadoProvider {
    store: Store,
    syncer: Syncer,
    verifier: Verifier,
    provider: DynProvider,

    pools: Arc<Mutex<Vec<PoolProvider>>>,
    known_pools_loaded: Arc<OnceCell<()>>,
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
            known_pools_loaded: Arc::new(OnceCell::new()),
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
        self.ensure_known_pools_loaded().await;

        for provider in self.pools.lock().await.iter() {
            provider.sync().await?;
        }
        Ok(())
    }

    /// Create a deposit for the given pool.
    pub async fn deposit(&self, pool: Pool, rng: &mut impl CryptoRng) -> Deposit {
        self.provider(&pool).await;
        Deposit::new(self.clone(), pool, rng.random(), rng.random())
    }

    /// Create a withdrawal for the given note.
    ///
    /// # Errors
    /// Returns an error if the note's pool cannot be found.
    pub fn withdraw(&self, note: Note, recipient: Address) -> Withdrawal {
        Withdrawal::new(self.clone(), note, recipient)
    }

    /// Quote the amount of fee token from a given wei amount.
    ///
    /// # Errors
    /// Returns an error if a contract call fails.
    pub async fn quote_wei_in_fee_token(
        &self,
        pool: &Pool,
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
        pool: &Pool,
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
        Ok(self.provider(&pool).await)
    }

    /// Get the pool for a given note.
    ///
    /// Searches both this provider's registered pools and the set of known pools for a pool
    /// matching the note's parameters.
    pub(crate) async fn pool_from_note(&self, note: &Note) -> Result<Pool, TornadoProviderError> {
        self.ensure_known_pools_loaded().await;

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
    ///
    /// Newly-seen pools are persisted to `store` so a future `TornadoProvider` over the same
    /// store remembers them automatically.
    async fn provider(&self, pool: &Pool) -> PoolProvider {
        self.ensure_known_pools_loaded().await;

        let mut pools = self.pools.lock().await;

        if let Some(p) = pools.iter().find(|p| &p.pool() == pool) {
            return p.clone();
        }

        let provider = self.build_pool_provider(pool.clone());
        pools.push(provider.clone());
        drop(pools);

        self.persist_known_pool(pool).await;
        provider
    }

    /// Loads the persisted set of known pools (once per `TornadoProvider`) and registers each
    /// one, so `sync`/`provider` find pools that were used in a previous session without
    /// needing to be touched again first.
    async fn ensure_known_pools_loaded(&self) {
        self.known_pools_loaded
            .get_or_init(|| async {
                let mut pools = self.pools.lock().await;
                for pool in known_pools::load(&self.store).await {
                    if pools.iter().any(|p| p.pool() == pool) {
                        continue;
                    }
                    pools.push(self.build_pool_provider(pool));
                }
            })
            .await;
    }

    /// Persists `pool` into the known-pools set if it isn't already there. Best-effort: a
    /// storage failure is logged, not propagated, since it only degrades the
    /// "remembered next time" convenience.
    async fn persist_known_pool(&self, pool: &Pool) {
        let mut known = known_pools::load(&self.store).await;
        if known.iter().any(|p| p == pool) {
            return;
        }

        known.push(pool.clone());
        known_pools::save(&self.store, &known).await;
    }

    fn build_pool_provider(&self, pool: Pool) -> PoolProvider {
        let scope = self.store.scope(pool.id());
        let indexer = Indexer::new(pool, scope, self.syncer.clone(), self.verifier.clone());
        PoolProvider::new(indexer, self.provider.clone())
    }
}
