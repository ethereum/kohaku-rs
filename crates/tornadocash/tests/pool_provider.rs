use alloy::{
    primitives::Address,
    providers::{Provider, ProviderBuilder, ext::AnvilApi},
    rpc::types::anvil::ReorgOptions,
    signers::local::PrivateKeySigner,
};
use kohaku_fork_kit::pool::deploy_pool;
use kohaku_kv_store::memory::MemoryStore;
use kohaku_tornadocash::{
    circuit::Circuit, indexer::rpc::RpcSyncer, provider::pool_provider::PoolProvider,
};
use tracing::info;

#[tokio::test]
#[ignore = "run with `cargo test --release -- --ignored`"]
async fn test_pool_provider() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init()
        .ok();

    let provider = ProviderBuilder::new().connect_anvil_with_wallet().erased();
    let pool = deploy_pool(provider.clone()).await?;

    let store = MemoryStore::new();
    let syncer = RpcSyncer::new(provider.clone()).with_batch_size(10_000);
    let circuit = Circuit::from_remote().await?;
    let mut pool_provider = PoolProvider::new(
        pool,
        provider.clone(),
        store.into(),
        syncer.clone().into(),
        syncer.clone().into(),
        circuit,
    );
    info!("Syncing pool provider");
    pool_provider.sync().await?;

    info!("Depositing into pool");
    let (deposit_call, note) = pool_provider.deposit(&mut rand::rng());
    info!("Deposit call: {deposit_call:?}");
    info!("Deposit note: {note:?}");

    let receipt = provider
        .send_transaction(deposit_call.into())
        .await?
        .get_receipt()
        .await?;
    info!("Deposit tx receipt: {receipt:?}");

    pool_provider.sync().await?;

    info!("Withdrawing from pool");
    let recipient: Address = PrivateKeySigner::random().address();
    let withdraw_call = pool_provider
        .withdraw(&note, recipient, None, None, None, &mut rand::rng())
        .await?;
    info!("Withdraw call: {withdraw_call:?}");

    let receipt = provider
        .send_transaction(withdraw_call.into())
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

    let store = MemoryStore::new();
    let syncer = RpcSyncer::new(provider.clone()).with_batch_size(10_000);
    let circuit = Circuit::from_remote().await?;
    let mut pool_provider = PoolProvider::new(
        pool,
        provider.clone(),
        store.into(),
        syncer.clone().into(),
        syncer.clone().into(),
        circuit,
    );
    pool_provider.sync().await?;

    info!("Depositing note_a");
    let (deposit_call_a, note_a) = pool_provider.deposit(&mut rand::rng());
    let receipt_a = provider
        .send_transaction(deposit_call_a.into())
        .await?
        .get_receipt()
        .await?;

    info!("Syncing pool provider after note_a deposit");
    pool_provider.sync().await?;

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
    let (deposit_call_b, note_b) = pool_provider.deposit(&mut rand::rng());
    provider
        .send_transaction(deposit_call_b.into())
        .await?
        .get_receipt()
        .await?;

    info!("Syncing pool provider after reorg");
    pool_provider.sync().await?;

    info!("Withdrawing note_b");
    let recipient: Address = PrivateKeySigner::random().address();
    let withdraw_call = pool_provider
        .withdraw(&note_b, recipient, None, None, None, &mut rand::rng())
        .await?;
    provider
        .send_transaction(withdraw_call.into())
        .await?
        .get_receipt()
        .await?;

    let withdraw_a = pool_provider
        .withdraw(&note_a, recipient, None, None, None, &mut rand::rng())
        .await;
    assert!(
        withdraw_a.is_err(),
        "note_a's commitment should have been replaced by the reorg"
    );

    Ok(())
}
