use alloy::providers::{Provider, ProviderBuilder};
use kohaku_kv_store::memory::MemoryStore;
use kohaku_tornadocash::{
    circuit::Circuit, indexer::rpc::RpcSyncer, provider::pool_provider::PoolProvider,
};
use tracing::info;

mod common;

#[tokio::test]
#[ignore = "run with `cargo test --release -- --ignored`"]
async fn test_sync() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_test_writer()
        .try_init()
        .ok();

    let provider = ProviderBuilder::new().connect_anvil_with_wallet().erased();
    let pool = common::local_chain::deploy_local_pool(provider.clone()).await?;

    let store = MemoryStore::new();
    let syncer = RpcSyncer::new(provider.clone());
    let circuit = Circuit::from_remote().await?;
    let mut pool_provider = PoolProvider::new(
        pool,
        provider.clone(),
        store.into(),
        syncer.clone().into(),
        syncer.clone().into(),
        circuit,
    );

    // Populate many arbitrary deposits
    for _ in 0..50 {
        let (deposit_call, _) = pool_provider.deposit(&mut rand::rng());
        provider
            .send_transaction(deposit_call.into())
            .await?
            .get_receipt()
            .await?;
    }

    info!("Syncing pool provider");
    pool_provider.sync().await?;

    Ok(())
}
