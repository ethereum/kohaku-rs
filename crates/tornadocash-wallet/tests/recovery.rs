use alloy::{
    primitives::Address,
    providers::{Provider, ProviderBuilder},
    signers::local::PrivateKeySigner,
};
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
async fn test_recovers_notes_from_the_keychain() -> Result<(), anyhow::Error> {
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
        syncer.clone().into(),
        provider.clone(),
    );

    let wallet = Wallet::new(
        tornado.clone(),
        common::TrivialKeychain.into(),
        Store::create(),
    );
    for nonce in 0..4u64 {
        let deposit = tornado
            .deposit(pool.clone(), &mut rand::rng())
            .await
            .with_wallet(&wallet)
            .await?;

        // Deposit all except nonce 2, which will be pending.
        if nonce != 2 {
            provider
                .send_transaction(deposit.clone().into())
                .await?
                .get_receipt()
                .await?;
        }

        // Spend nonce 0.
        if nonce == 0 {
            tornado.sync().await?;
            let recipient: Address = PrivateKeySigner::random().address();
            provider
                .send_transaction(
                    tornado
                        .withdraw(deposit.note(), recipient)
                        .into_transaction(&mut rand::rng())
                        .await?,
                )
                .await?
                .get_receipt()
                .await?;
        }
    }

    // A fresh wallet over an empty store.
    let recovered_tornado = TornadoProvider::new(
        Store::create(),
        syncer.clone().into(),
        syncer.clone().into(),
        provider.clone(),
    );
    let recovered = Wallet::new(
        recovered_tornado.clone(),
        common::TrivialKeychain.into(),
        Store::create(),
    );
    let notes = recovered.notes(&pool).await?;

    assert_eq!(notes.len(), 4);
    assert!(matches!(notes[0].status, NoteStatus::Withdrawn { .. }),);
    assert!(matches!(notes[1].status, NoteStatus::Deposited { .. }));
    assert!(matches!(notes[2].status, NoteStatus::Pending));
    assert!(matches!(notes[3].status, NoteStatus::Deposited { .. }));

    Ok(())
}

#[tokio::test]
#[ignore = "run with `cargo test --release -- --ignored`"]
async fn test_recovery_stops_at_the_gap_limit() -> Result<(), anyhow::Error> {
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
    for nonce in 0..4u64 {
        let deposit = tornado
            .deposit(pool.clone(), &mut rand::rng())
            .await
            .with_wallet(&wallet)
            .await?;

        // Deposit nonces 0 and 3, leaving a gap at 1 and 2.
        if nonce == 0 || nonce == 3 {
            provider
                .send_transaction(deposit.into())
                .await?
                .get_receipt()
                .await?;
        }
    }

    // Attempt to recover notes with a max gap limit of 2.
    let recovered = Wallet::new(
        tornado.clone(),
        common::TrivialKeychain.into(),
        Store::create(),
    )
    .with_gap_limit(2);

    let notes = recovered.notes(&pool).await?;
    assert_eq!(notes.len(), 1);

    Ok(())
}
