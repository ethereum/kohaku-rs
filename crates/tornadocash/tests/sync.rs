use alloy::providers::{Provider, ProviderBuilder};
use kohaku_fork_kit::pool::deploy_pool;
use kohaku_kv_store::Store;
use kohaku_tornadocash::{indexer::rpc::RpcSyncer, provider::TornadoProvider};
use tracing::info;

#[tokio::test]
#[ignore = "run with `cargo test --release -- --ignored`"]
async fn test_sync() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_test_writer()
        .try_init()
        .ok();

    let provider = ProviderBuilder::new().connect_anvil_with_wallet().erased();
    let pool = deploy_pool(provider.clone(), None).await?;

    let store = Store::create();
    let syncer = RpcSyncer::new(provider.clone());
    let tornado_provider = TornadoProvider::new(
        store,
        syncer.clone().into(),
        syncer.clone().into(),
        provider.clone(),
    );

    // Populate many arbitrary deposits
    for _ in 0..50 {
        let deposit = tornado_provider.deposit(pool.clone(), &mut rand::rng()).await;
        provider
            .send_transaction(deposit.into())
            .await?
            .get_receipt()
            .await?;
    }

    info!("Syncing provider");
    tornado_provider.sync().await?;

    Ok(())
}
