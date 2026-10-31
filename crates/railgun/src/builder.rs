use std::sync::Arc;

use alloy::providers::{DynProvider, Provider};

use crate::{
    chain_config::ChainConfig,
    circuit::groth16_prover::Groth16Prover,
    indexer::{
        syncer::{ChainedSyncer, RpcSyncer, SubsquidSyncer, UtxoSyncer},
        utxo_indexer::UtxoIndexer,
    },
    merkle_tree::SmartWalletUtxoVerifier,
    poi::provider::PoiProvider,
    state::RailgunCheckpoint,
    wallet::{RailgunError, RailgunWallet},
};

/// Builder for constructing a `RailgunWallet`.
pub struct RailgunBuilder {
    chain: ChainConfig,
    provider: DynProvider,
    state: RailgunCheckpoint,
    utxo_syncer: Option<Arc<dyn UtxoSyncer>>,
    poi: bool,
}

impl RailgunBuilder {
    #[must_use]
    pub fn new(chain: ChainConfig, provider: impl Provider + 'static) -> Self {
        Self {
            chain,
            provider: provider.erased(),
            state: RailgunCheckpoint::default(),
            utxo_syncer: None,
            poi: false,
        }
    }

    /// Supplies caller-owned state from a previous session. The application
    /// decides how to serialize and persist it.
    #[must_use]
    pub fn with_state(mut self, state: RailgunCheckpoint) -> Self {
        self.state = state;
        self
    }

    /// Sets a custom UTXO syncer for the wallet. If not set, a default subsquid + RPC syncer will
    /// be used.
    #[must_use]
    pub fn with_utxo_syncer(mut self, syncer: Arc<dyn UtxoSyncer>) -> Self {
        self.utxo_syncer = Some(syncer);
        self
    }

    /// Enables POI support.
    ///
    /// Uses the chain's POI endpoint and list keys. Spending selects only notes
    /// whose local status is `Valid`. [`RailgunWallet::sync`] and
    /// [`RailgunWallet::sync_to`] copy the list, then
    /// [`RailgunWallet::process_pending_poi`] submits proofs.
    #[must_use]
    pub fn with_poi(mut self) -> Self {
        self.poi = true;
        self
    }

    /// Builds the `RailgunWallet` with the specified configuration.
    #[must_use]
    #[expect(clippy::unused_async, reason = "Preserve the builder's async API")]
    pub async fn build(self) -> Result<RailgunWallet, RailgunError> {
        let utxo_syncer = self.utxo_syncer.unwrap_or_else(|| {
            Arc::new(
                ChainedSyncer::new()
                    .then(SubsquidSyncer::new(&self.chain.subsquid_endpoint))
                    .then(RpcSyncer::new(self.chain.clone(), self.provider.clone())),
            )
        });

        let utxo_verifier = Arc::new(SmartWalletUtxoVerifier::new(
            self.chain.railgun_smart_wallet,
            self.provider.clone(),
        ));

        let utxo_indexer = UtxoIndexer::new(self.state.utxo, utxo_syncer, utxo_verifier)?;

        let prover = Groth16Prover::new();

        let (active_poi, preserved_poi) = if self.poi {
            (self.state.poi, Default::default())
        } else {
            (Default::default(), self.state.poi)
        };
        let poi_provider = if self.poi {
            let txid_syncer = Arc::new(SubsquidSyncer::new(&self.chain.subsquid_endpoint));

            let poi_provider = PoiProvider::new(
                self.chain.id,
                active_poi,
                txid_syncer,
                self.chain.poi_endpoint.clone(),
                self.chain.list_keys.clone(),
            )?;
            Some(poi_provider)
        } else {
            None
        };

        RailgunWallet::new(
            self.chain,
            self.provider,
            utxo_indexer,
            prover,
            poi_provider,
            preserved_poi,
        )
    }
}
