#![doc = include_str!("../README.md")]

pub mod client;
#[cfg(all(test, native))]
mod conformance_tests;
pub mod lists;
pub mod poi_inputs;
pub mod poi_note;
pub mod poi_tree;
pub mod provider;
pub mod txid_indexer;
pub mod txid_tree;
pub mod types;
pub mod wallet;

pub mod account {
    pub use kohaku_railgun::account::*;
}
pub mod builder {
    pub use kohaku_railgun::builder::*;
}
pub mod caip {
    pub use kohaku_railgun::caip::*;
}
pub mod chain_config {
    pub use kohaku_railgun::chain_config::*;
}
pub mod state {
    pub use kohaku_railgun::state::*;
}

pub use client::{PoiClientError, PoiNodeClient};
pub use lists::{PoiListState, PoiLists, PoiListsState, PoiSyncError};
pub use provider::{
    PendingPoiSummary, PoiCheckpoint, PoiProcessFailure, PoiProcessReport, PoiProvider,
    PoiProviderError, PoiProviderState,
};
pub use txid_indexer::{TxidIndexer, TxidIndexerError, TxidIndexerState};

pub mod circuit {
    pub mod groth16_prover {
        pub use kohaku_railgun::circuit::groth16_prover::*;
    }
    pub mod inputs {
        pub mod poi_inputs {
            pub use crate::poi_inputs::*;
        }
    }
}
pub mod crypto {
    pub use kohaku_railgun::crypto::*;
}
pub mod indexer {
    pub mod indexed_account {
        pub use kohaku_railgun::indexer::IndexedAccountState;
    }
    pub mod syncer {
        pub use kohaku_railgun::indexer::syncer::*;
    }
    pub mod txid_indexer {
        pub use crate::txid_indexer::*;
    }
    pub mod utxo_indexer {
        pub use kohaku_railgun::indexer::{UtxoIndexer, UtxoIndexerState};
    }
}
pub mod merkle_tree {
    pub use crate::poi_tree::{PoiHasher, PoiMerkleTree, is_poi_empty_leaf};
    pub use crate::txid_tree::{TxidLeafHash, TxidMerkleTree, UtxoTreeIndex};
    pub use kohaku_railgun::merkle_tree::*;
}
pub mod note {
    pub use kohaku_railgun::note::*;
}
pub mod transact {
    pub use kohaku_railgun::transact::*;
}
pub mod poi {
    pub mod client {
        pub use crate::client::*;
    }
    pub mod lists {
        pub use crate::lists::*;
    }
    pub mod note {
        pub use crate::poi_note::*;
    }
    pub mod provider {
        pub use crate::provider::*;
    }
    #[cfg(all(test, native))]
    pub(crate) mod conformance_tests {
        pub(crate) use crate::conformance_tests::assert_fresh_submission;
    }
    pub mod recovery {
        pub use kohaku_railgun::indexer::recovery::*;
    }
    pub mod types {
        pub use crate::types::*;
    }
}
pub mod common {
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn sleep(duration: web_time::Duration) {
        tokio::time::sleep(duration).await;
    }
    #[cfg(target_arch = "wasm32")]
    pub async fn sleep(duration: web_time::Duration) {
        gloo_timers::future::sleep(duration).await;
    }
}
