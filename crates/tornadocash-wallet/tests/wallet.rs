use alloy::providers::{Provider, ProviderBuilder};
use kohaku_fork_kit::pool::deploy_pool;
use kohaku_kv_store::Store;
use kohaku_tornadocash::{indexer::rpc::RpcSyncer, provider::TornadoProvider};
use kohaku_tornadocash_wallet::{
    ext::DepositWalletExt,
    wallet::{NoteStatus, Wallet},
};

mod common;

#[tokio::test]
#[ignore = "run with `cargo test --release -- --ignored`"]
async fn test_reserved_notes_are_pending() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_test_writer()
        .try_init()
        .ok();

    let provider = ProviderBuilder::new().connect_anvil_with_wallet().erased();
    let pool = deploy_pool(provider.clone(), None).await?;

    let syncer = RpcSyncer::new(provider.clone());
    let tornado = TornadoProvider::new(
        Store::create(),
        syncer.clone().into(),
        syncer.into(),
        provider.clone(),
    );
    let wallet = Wallet::new(
        tornado.clone(),
        common::TrivialKeychain.into(),
        Store::create(),
    );

    wallet.reserve(&pool).await?;

    let notes = wallet.notes(&pool).await?;
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].status, NoteStatus::Pending);

    Ok(())
}

#[tokio::test]
#[ignore = "run with `cargo test --release -- --ignored`"]
async fn test_with_wallet_reserves_nonce() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_test_writer()
        .try_init()
        .ok();

    let provider = ProviderBuilder::new().connect_anvil_with_wallet().erased();
    let pool = deploy_pool(provider.clone(), None).await?;

    let syncer = RpcSyncer::new(provider.clone());
    let tornado = TornadoProvider::new(
        Store::create(),
        syncer.clone().into(),
        syncer.into(),
        provider.clone(),
    );
    let wallet = Wallet::new(
        tornado.clone(),
        common::TrivialKeychain.into(),
        Store::create(),
    );

    let deposit = tornado
        .deposit(pool.clone(), &mut rand::rng())
        .await
        .with_wallet(&wallet)
        .await?;

    assert_eq!(deposit.note(), wallet.note(&pool, 0).await?.note);
    assert_eq!(wallet.reserve(&pool).await.unwrap().0, 1);

    Ok(())
}

#[tokio::test]
#[ignore = "run with `cargo test --release -- --ignored`"]
async fn test_pools_are_scoped() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_test_writer()
        .try_init()
        .ok();

    let provider = ProviderBuilder::new().connect_anvil_with_wallet().erased();
    let pool = deploy_pool(provider.clone(), None).await?;
    let other = deploy_pool(provider.clone(), Some(123_456)).await?;

    let syncer = RpcSyncer::new(provider.clone());
    let tornado = TornadoProvider::new(
        Store::create(),
        syncer.clone().into(),
        syncer.into(),
        provider.clone(),
    );
    let wallet = Wallet::new(
        tornado.clone(),
        common::TrivialKeychain.into(),
        Store::create(),
    );

    assert_eq!(wallet.reserve(&pool).await.unwrap().0, 0);
    assert_eq!(wallet.reserve(&pool).await.unwrap().0, 1);
    assert_eq!(wallet.reserve(&other).await.unwrap().0, 0,);

    Ok(())
}
