#![doc = include_str!("../README.md")]

mod chained;
mod log_decode;
mod normalize_tree_position;
mod rpc;
mod subsquid;
mod subsquid_types;

pub use chained::ChainedSyncer;
pub use log_decode::log_to_sync_events;
pub use rpc::RpcSyncer;
pub use subsquid::SubsquidSyncer;

use alloy::providers::Provider;
use kohaku_railgun::chain_config::ChainConfig;

#[cfg(not(target_arch = "wasm32"))]
async fn sleep(duration: web_time::Duration) {
    tokio::time::sleep(duration).await;
}

#[cfg(target_arch = "wasm32")]
async fn sleep(duration: web_time::Duration) {
    gloo_timers::future::sleep(duration).await;
}

/// The usual Subsquid history followed by an RPC tail.
pub fn default_utxo_syncer(
    chain: &ChainConfig,
    provider: impl Provider + 'static,
) -> ChainedSyncer {
    ChainedSyncer::new()
        .then(SubsquidSyncer::new(&chain.subsquid_endpoint))
        .then(RpcSyncer::new(chain.clone(), provider))
}
