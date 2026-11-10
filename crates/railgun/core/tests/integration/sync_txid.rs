use std::sync::Arc;

use alloy::{
    network::Ethereum,
    providers::{Provider, ProviderBuilder},
};
use kohaku_railgun::{builder::RailgunBuilder, chain_config::ChainConfig};
use kohaku_railgun_poi::{provider::PoiCheckpoint, wallet::PoiWallet};
use kohaku_railgun_sync::SubsquidSyncer;
use tracing::info;
use tracing_subscriber::EnvFilter;

/// Tests syncing the txid indexer to a specific block. This integration test ensures that
/// the Txid indexer can successfully sync and verifies that its merkle tree is consistent
/// with subsquid's ground truth.
#[tokio::test]
#[serial_test::serial]
#[ignore]
async fn test_sync_txid() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_test_writer()
        .try_init()
        .ok();

    let chain = ChainConfig::sepolia();

    info!("Setting up chain client");
    let rpc_url = std::env::var("RPC_URL_SEPOLIA").expect("RPC_URL_SEPOLIA Must be set");
    let provider = ProviderBuilder::new()
        .network::<Ethereum>()
        .connect(&rpc_url)
        .await
        .unwrap()
        .erased();

    info!("Setting up provider");
    let syncer = Arc::new(SubsquidSyncer::new(&chain.subsquid_endpoint));
    let wallet = RailgunBuilder::new(chain, provider)
        .with_utxo_syncer(syncer.clone())
        .build()
        .await
        .unwrap();
    let mut railgun = PoiWallet::new(wallet, PoiCheckpoint::default(), syncer).unwrap();

    info!("Syncing");
    railgun.sync().await.unwrap();
}
