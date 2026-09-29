use std::ops::{Deref, RangeBounds};

use alloy::{
    primitives::{Address, B256},
    providers::DynProvider,
};
use ruint::aliases::U256;
use thiserror::Error;

use crate::{
    abis::tornado::Tornado::{self, TornadoInstance},
    pool::{Asset, Pool},
    syncer::{Synced, Syncer},
};

/// A provider for interacting with Tornado Cash pools.
#[derive(Clone)]
pub struct TornadoProvider {
    provider: DynProvider,
    syncer: Syncer,
}

#[derive(Debug, Error)]
pub enum TornadoProviderError {
    #[error("Provider error: {0}")]
    Provider(#[from] alloy::contract::Error),
    #[error("Syncer error: {0}")]
    Syncer(#[from] crate::syncer::SyncerError),
}

impl TornadoProvider {
    #[must_use]
    pub fn new(provider: DynProvider, syncer: Syncer) -> Self {
        Self { provider, syncer }
    }

    /// Syncs the given pool, returning the events within `range` and the range actually covered.
    ///
    /// # Errors
    /// Returns an error if the syncer fails to fetch the events.
    pub async fn sync(
        &self,
        pool: &Pool,
        range: impl RangeBounds<u64>,
    ) -> Result<Synced, TornadoProviderError> {
        Ok(self.syncer.sync(pool, range).await?)
    }

    /// Returns whether `root` is present in the pool's on-chain root history.
    ///
    /// # Errors
    /// Returns an error if a contract call fails.
    pub async fn is_known_root(
        &self,
        pool: &Pool,
        root: B256,
    ) -> Result<bool, TornadoProviderError> {
        self.tornado(pool)
            .isKnownRoot(root)
            .call()
            .await
            .map_err(TornadoProviderError::from)
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
        match pool.asset {
            Asset::Native { .. } => Ok(wei_amount),
            Asset::Erc20 { address, .. } => {
                self.quote_wei_in_token(pool, address, wei_amount).await
            }
        }
    }

    /// Check if a note has been spent.
    ///
    /// # Errors
    /// Returns an error if a contract call fails.
    pub async fn is_spent(
        &self,
        pool: &Pool,
        nullifier_hash: B256,
    ) -> Result<bool, TornadoProviderError> {
        self.tornado(pool)
            .isSpent(nullifier_hash)
            .call()
            .await
            .map_err(TornadoProviderError::from)
    }

    async fn quote_wei_in_token(
        &self,
        pool: &Pool,
        token_address: Address,
        wei_amount: U256,
    ) -> Result<U256, TornadoProviderError> {
        self.tornado(pool)
            .quoteWeiInToken(token_address, wei_amount)
            .call()
            .await
            .map_err(TornadoProviderError::from)
    }

    fn tornado(&self, pool: &Pool) -> TornadoInstance<DynProvider> {
        Tornado::new(pool.address, self.provider.clone())
    }
}

impl Deref for TornadoProvider {
    type Target = DynProvider;

    fn deref(&self) -> &Self::Target {
        &self.provider
    }
}
