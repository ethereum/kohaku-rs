use alloy::{
    node_bindings::Anvil,
    providers::{Provider, ProviderBuilder},
    signers::local::PrivateKeySigner,
};
use kohaku_fork_kit::{
    pool::{deploy_pool, deploy_proxy},
    relayer::RelayerBuilder,
};
use kohaku_kv_store::Store;
use kohaku_tornadocash::{indexer::rpc::RpcSyncer, provider::TornadoProvider, relayer::Relayer};
use tracing::info;

#[tokio::test]
#[ignore = "run with `cargo test --release -- --ignored`"]
async fn test_relayer_withdraw() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init()
        .ok();

    let anvil = Anvil::new().block_time(1).try_spawn()?;
    let wallet = anvil.wallet().expect("anvil provides a dev wallet");
    let provider = ProviderBuilder::new()
        .wallet(wallet)
        .connect_http(anvil.endpoint_url())
        .erased();

    let pool = deploy_pool(provider.clone()).await?;
    let proxy_address = deploy_proxy(provider.clone()).await?;

    let store = Store::create();
    let syncer = RpcSyncer::new(provider.clone());
    let tornado_provider = TornadoProvider::new(
        store,
        syncer.clone().into(),
        syncer.clone().into(),
        provider.clone(),
    );

    info!("Depositing into pool");
    let (deposit_call, note) = tornado_provider.deposit(pool, &mut rand::rng()).await;
    provider
        .send_transaction(deposit_call.into())
        .await?
        .get_receipt()
        .await?;
    tornado_provider.sync().await?;

    info!("Starting local tornado-relayer");
    let relayer_signer = PrivateKeySigner::random();
    let reward_account = PrivateKeySigner::random().address();
    let relayer_instance = RelayerBuilder::new(
        anvil.endpoint(),
        anvil.ws_endpoint(),
        pool,
        proxy_address,
        relayer_signer.to_bytes().to_string(),
        reward_account,
    )
    .prefund(&provider)
    .await?
    .spawn()
    .await?;

    let relayer = Relayer::from_client(relayer_instance.clone());

    let fee = relayer
        .estimate_fee(&tornado_provider, &note, None)
        .await
        .expect("relayer should quote a fee for a supported pool");
    info!("Relayer quoted fee: {fee}");

    info!("Building withdrawal, relaying via {relayer_signer:?}");
    let recipient = PrivateKeySigner::random().address();
    let receipt = relayer
        .withdraw(&tornado_provider, &note, recipient, None, &mut rand::rng())
        .await?;
    info!("Relayer accepted withdrawal job {receipt:?}");

    let tx_hash = relayer
        .await_confirmation(&tornado_provider, &receipt)
        .await?;
    assert!(
        tx_hash.is_some(),
        "relayer should have confirmed the withdrawal on-chain"
    );

    Ok(())
}
