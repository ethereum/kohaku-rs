use alloy::{
    node_bindings::Anvil,
    primitives::address,
    providers::{Provider, ProviderBuilder},
    signers::local::PrivateKeySigner,
};
use kohaku_kv_store::memory::MemoryStore;
use kohaku_tornadocash::{
    circuit::Circuit, indexer::rpc::RpcSyncer, provider::tornado_provider::TornadoProvider,
    userop_provider::TornadoPaymasterExt,
};
use kohaku_userop_kit::{
    builder::UserOperationBuilder,
    entry_point::ENTRY_POINT_08,
    smart_account::simple_7702_smart_account::{Call, Simple7702SmartAccount},
};
use tracing::info;

mod common;

const ALTO_EXECUTOR_PK: &str = "0x4a3a02862ddcb260ed52d40ef03f8e3d78fa3d174b0ef333afdf1ffb4a648cd5";
const ALTO_UTILITY_PK: &str = "0xdd4b2564c83ff7de602c39ffda1146055dc1814b07c083d7971722384f1f01a6";

#[tokio::test]
#[ignore = "run with `cargo test --release -- --ignored`"]
async fn test_tornadocash_paymaster() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init()
        .ok();

    let anvil: alloy::node_bindings::AnvilInstance = Anvil::new().try_spawn()?;
    let wallet = anvil.wallet().expect("anvil provides a dev wallet");
    let provider = ProviderBuilder::new()
        .wallet(wallet)
        .connect_http(anvil.endpoint_url())
        .erased();
    let chain_id = provider.get_chain_id().await?;

    let pool =
        common::local_paymaster_chain::deploy_local_pool_with_paymaster(provider.clone()).await?;

    let store = MemoryStore::new();
    let syncer = RpcSyncer::new(provider.clone()).with_batch_size(10_000);
    let circuit = Circuit::from_remote().await?;
    let mut tornado_provider = TornadoProvider::new(
        provider.clone(),
        store.into(),
        syncer.clone().into(),
        syncer.clone().into(),
        circuit,
    );

    info!("Syncing pool provider");
    tornado_provider.pool(pool);
    tornado_provider.sync().await?;

    info!("Depositing into pool");
    let (deposit_call, note) = tornado_provider.deposit(pool, &mut rand::rng())?;
    info!("Deposit call: {deposit_call:?}");
    info!("Deposit note: {note:?}");

    provider
        .send_transaction(deposit_call.into())
        .await?
        .get_receipt()
        .await?;
    tornado_provider.sync().await?;

    info!("Starting local alto bundler");
    let alto = common::alto::AltoBuilder::new(
        anvil.endpoint_url().to_string(),
        ENTRY_POINT_08,
        ALTO_EXECUTOR_PK,
        ALTO_UTILITY_PK,
    )
    .prefund(&provider)
    .await?
    .spawn()
    .await?;

    info!("Withdrawing from pool with tornadocash paymaster");
    let owner = PrivateKeySigner::random();
    let smart_account = Simple7702SmartAccount::new(provider.clone(), owner.address(), chain_id);

    let userop = UserOperationBuilder::new_with_smart_account(&smart_account)
        .await?
        .with_call(&vec![Call {
            target: address!("0x000000000000000000000000000000000000dead"),
            ..Default::default()
        }])
        .with_tornadocash_paymaster(
            &note,
            owner.address(),
            &mut tornado_provider,
            &*alto,
            &mut rand::rng(),
        )
        .await?
        .build()
        .sign(&owner)
        .await?;

    let userop_hash = alto.send_user_operation(&userop).await?;
    let userop_receipt = alto.wait_for_receipt(userop_hash).await?;
    info!("Userop receipt: {userop_receipt:?}");

    assert!(userop_receipt.success, "userop should succeed");
    Ok(())
}
