//! Typed Railgun checkpoints. Callers own serialization and persistence.

use serde::{Deserialize, Serialize};

use crate::{indexer::utxo_indexer::UtxoIndexerState, poi::provider::PoiCheckpoint};

/// State needed to resume a Railgun session. Keep one checkpoint per chain and wallet.
/// Account notes and pending POI inputs are sensitive wallet data.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct RailgunCheckpoint {
    pub utxo: UtxoIndexerState,
    pub poi: PoiCheckpoint,
}
