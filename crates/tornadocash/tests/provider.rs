use alloy::{
    primitives::Address,
    providers::{Provider, ProviderBuilder, ext::AnvilApi},
    rpc::types::anvil::ReorgOptions,
    signers::local::PrivateKeySigner,
};
use kohaku_fork_kit::pool::deploy_pool;
use kohaku_kv_store::Store;
use kohaku_tornadocash::{indexer::rpc::RpcSyncer, provider::TornadoProvider};
use tracing::info;

#[tokio::test]
#[ignore = "run with `cargo test --release -- --ignored`"]
async fn test_provider() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init()
        .ok();

    let provider = ProviderBuilder::new().connect_anvil_with_wallet().erased();
    let pool = deploy_pool(provider.clone()).await?;

    let syncer = RpcSyncer::new(provider.clone());
    let tornado_provider = TornadoProvider::new(
        Store::create(),
        syncer.clone().into(),
        syncer.clone().into(),
        provider.clone(),
    );

    info!("Depositing into pool");
    let (deposit_call, note) = tornado_provider.deposit(pool, &mut rand::rng()).await;
    info!("Deposit call: {deposit_call:?}");
    info!("Deposit note: {note:?}");

    info!("Syncing pool provider");
    tornado_provider.sync().await?;

    let receipt = provider
        .send_transaction(deposit_call.into())
        .await?
        .get_receipt()
        .await?;
    info!("Deposit tx receipt: {receipt:?}");

    tornado_provider.sync().await?;

    info!("Withdrawing from pool");
    let recipient: Address = PrivateKeySigner::random().address();
    let withdrawal = tornado_provider
        .withdraw(&note, recipient, None, None, None, &mut rand::rng())
        .await?;
    info!("Withdraw call: {withdrawal:?}");

    let receipt = provider
        .send_transaction(withdrawal.into())
        .await?
        .get_receipt()
        .await?;
    info!("Withdraw tx receipt: {receipt:?}");

    Ok(())
}

#[tokio::test]
#[ignore = "run with `cargo test --release -- --ignored`"]
async fn test_pool_provider_reorg_recovery() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init()
        .ok();

    // Use `SimpleNonceManager` instead of the default `CachedNonceManager` so the cached
    // nonce manager doesn't break after reorgs.
    let provider = ProviderBuilder::new()
        .disable_recommended_fillers()
        .with_gas_estimation()
        .fetch_chain_id()
        .with_simple_nonce_management()
        .connect_anvil_with_wallet()
        .erased();
    let pool = deploy_pool(provider.clone()).await?;

    let syncer = RpcSyncer::new(provider.clone());
    let tornado_provider = TornadoProvider::new(
        Store::create(),
        syncer.clone().into(),
        syncer.clone().into(),
        provider.clone(),
    );

    info!("Depositing note_a");
    let (deposit_call_a, note_a) = tornado_provider.deposit(pool, &mut rand::rng()).await;
    let receipt_a = provider
        .send_transaction(deposit_call_a.into())
        .await?
        .get_receipt()
        .await?;

    info!("Syncing provider after note_a deposit");
    tornado_provider.sync().await?;

    info!("Reorging out note_a's deposit");
    provider
        .anvil_reorg(ReorgOptions {
            depth: 1,
            tx_block_pairs: Vec::new(),
        })
        .await?;
    provider
        .anvil_drop_transaction(receipt_a.transaction_hash)
        .await?;

    info!("Depositing note_b in its place");
    let (deposit_call_b, note_b) = tornado_provider.deposit(pool, &mut rand::rng()).await;
    provider
        .send_transaction(deposit_call_b.into())
        .await?
        .get_receipt()
        .await?;

    info!("Syncing provider after reorg");
    tornado_provider.sync().await?;

    info!("Withdrawing note_b");
    let recipient: Address = PrivateKeySigner::random().address();
    let withdraw_call = tornado_provider
        .withdraw(&note_b, recipient, None, None, None, &mut rand::rng())
        .await?;
    provider
        .send_transaction(withdraw_call.into())
        .await?
        .get_receipt()
        .await?;

    let withdraw_a = tornado_provider
        .withdraw(&note_a, recipient, None, None, None, &mut rand::rng())
        .await;
    assert!(
        withdraw_a.is_err(),
        "note_a's commitment should have been replaced by the reorg"
    );

    Ok(())
}
