use std::sync::Arc;

use alloy::providers::{DynProvider, Provider};

use crate::{
    chain_config::ChainConfig,
    circuit::groth16_prover::Groth16Prover,
    indexer::{syncer::UtxoSyncer, utxo_indexer::UtxoIndexer},
    merkle_tree::SmartWalletUtxoVerifier,
    state::RailgunCheckpoint,
    wallet::{RailgunError, RailgunWallet},
};

/// Builder for constructing a `RailgunWallet`.
pub struct RailgunBuilder {
    chain: ChainConfig,
    provider: DynProvider,
    state: RailgunCheckpoint,
    utxo_syncer: Option<Arc<dyn UtxoSyncer>>,
    artifact_base_url: Option<String>,
}

impl RailgunBuilder {
    #[must_use]
    pub fn new(chain: ChainConfig, provider: impl Provider + 'static) -> Self {
        Self {
            chain,
            provider: provider.erased(),
            state: RailgunCheckpoint::default(),
            utxo_syncer: None,
            artifact_base_url: None,
        }
    }

    /// Supplies caller-owned state from a previous session. The application
    /// decides how to serialize and persist it.
    #[must_use]
    pub fn with_state(mut self, state: RailgunCheckpoint) -> Self {
        self.state = state;
        self
    }

    /// Supplies a UTXO syncer chosen by the consumer.
    #[must_use]
    pub fn with_utxo_syncer(mut self, syncer: Arc<dyn UtxoSyncer>) -> Self {
        self.utxo_syncer = Some(syncer);
        self
    }

    /// Select a caller-controlled circuit source (for example, packaged artifacts
    /// served by a JavaScript host). The default remains the pinned remote source.
    #[must_use]
    pub fn with_artifact_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.artifact_base_url = Some(base_url.into());
        self
    }

    /// Builds the `RailgunWallet` with the specified configuration.
    #[must_use]
    #[expect(clippy::unused_async, reason = "Preserve the builder's async API")]
    pub async fn build(self) -> Result<RailgunWallet, RailgunError> {
        let utxo_syncer = self.utxo_syncer.ok_or(RailgunError::MissingUtxoSyncer)?;

        let utxo_verifier = Arc::new(SmartWalletUtxoVerifier::new(
            self.chain.railgun_smart_wallet,
            self.provider.clone(),
        ));

        let utxo_indexer = UtxoIndexer::new(self.state.utxo, utxo_syncer, utxo_verifier)?;

        let prover = self
            .artifact_base_url
            .as_deref()
            .map_or_else(Groth16Prover::new, Groth16Prover::with_artifact_base_url);

        RailgunWallet::new(self.chain, utxo_indexer, prover)
    }
}
