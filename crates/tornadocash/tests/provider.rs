use alloy::{
    primitives::{Address, U256},
    providers::{Provider, ProviderBuilder},
    signers::local::PrivateKeySigner,
};
use kohaku_fork_kit::pool::deploy_pool;
use kohaku_kv_store::Store;
use kohaku_tornadocash::{
    deposit::Deposit,
    indexer::rpc::RpcSyncer,
    merkle_tree::{TcMerkleTree, TcMerkleTreeExt},
    provider::TornadoProvider,
    withdrawal::Withdrawal,
};

#[tokio::test]
#[ignore = "run with `cargo test --release -- --ignored`"]
async fn test_deposit() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init()
        .ok();

    let provider = ProviderBuilder::new().connect_anvil_with_wallet().erased();
    let pool = deploy_pool(provider.clone(), None).await?;

    // Deposit a note
    let deposit = Deposit::random(&pool, &mut rand::rng());
    provider
        .send_transaction(deposit.into())
        .await?
        .watch()
        .await?;

    // Assert recipient balance
    let balance = provider.get_balance(pool.address).await?;
    assert!(
        balance == U256::from(pool.amount_wei),
        "Pool balance should be non-zero"
    );

    Ok(())
}

#[tokio::test]
#[ignore = "run with `cargo test --release -- --ignored`"]
async fn test_withdraw() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init()
        .ok();

    let provider = ProviderBuilder::new().connect_anvil_with_wallet().erased();
    let pool = deploy_pool(provider.clone(), None).await?;

    // Deposit a note
    let deposit = Deposit::random(&pool, &mut rand::rng());
    let note = deposit.note();
    provider
        .send_transaction(deposit.into())
        .await?
        .watch()
        .await?;

    // Construct a TornadoProvider
    let syncer = RpcSyncer::new(provider.clone());
    let tornado_provider = TornadoProvider::new(syncer.clone().into(), provider.clone());

    // Sync a Merkle tree against the provider
    let tree = TcMerkleTree::new(Store::create());
    let synced = tornado_provider.sync(&pool, ..).await?;
    tree.splice_events(&synced.events).await?;

    // Withdraw the note
    let recipient: Address = PrivateKeySigner::random().address();
    let withdrawal = Withdrawal::new(&pool, note, recipient)
        .prove(&tree, &mut rand::rng())
        .await?;
    provider
        .send_transaction(withdrawal.into())
        .await?
        .watch()
        .await?;

    // Assert recipient balance
    let balance = provider.get_balance(recipient).await?;
    assert!(
        balance == U256::from(pool.amount_wei),
        "Recipient balance should be non-zero"
    );

    Ok(())
}
