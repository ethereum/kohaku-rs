use std::sync::Arc;

use alloy::{
    primitives::Address,
    providers::{Provider, ProviderBuilder},
    signers::local::PrivateKeySigner,
};
use kohaku_tornadocash::{
    circuit::Circuit, indexer::rpc::RpcSyncer, kv::MemoryKvStore,
    provider::pool_provider::PoolProvider,
};
use tracing::info;

mod common;

#[tokio::test]
#[ignore = "run with `cargo test --release -- --ignored`"]
async fn test_pool_provider() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init()
        .ok();

    let provider = ProviderBuilder::new().connect_anvil_with_wallet().erased();
    let pool = common::local_chain::deploy_local_pool(provider.clone()).await?;

    let syncer = Arc::new(RpcSyncer::new(provider.clone()).with_batch_size(10_000));
    let store = Arc::new(MemoryKvStore::default());
    let circuit = Circuit::from_remote().await?;
    let mut pool_provider = PoolProvider::new(
        pool,
        provider.clone(),
        store,
        syncer.clone(),
        syncer.clone(),
        circuit,
    )?;
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
