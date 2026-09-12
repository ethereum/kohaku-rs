use alloy::{
    node_bindings::Anvil,
    primitives::address,
    providers::{Provider, ProviderBuilder},
    signers::local::PrivateKeySigner,
};
use kohaku_fork_kit::{
    alto::AltoBuilder,
    entry_point::deploy_entry_point,
    paymaster::{deploy_fee_adapter, deploy_paymaster},
    pool::deploy_pool,
    simple_account::deploy_simple_account,
};
use kohaku_kv_store::memory::MemoryStore;
use kohaku_tornadocash::{
    circuit::Circuit, indexer::rpc::RpcSyncer, provider::tornado_provider::TornadoProvider,
    userop_provider::TornadoPaymasterExt,
};
use kohaku_userop_kit::{
    builder::UserOperationBuilder,
    smart_account::simple_7702_smart_account::{Call, Simple7702SmartAccount},
};
use tracing::info;

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

    let entrypoint = deploy_entry_point(&provider).await?;
    deploy_simple_account(&provider).await?;

    let mut pool = deploy_pool(provider.clone()).await?;

    let placeholder_factory = address!("0x0000000000000000000000000000000000000011");
    let placeholder_weth = address!("0x0000000000000000000000000000000000000022");
    let paymaster_address = deploy_paymaster(
        provider.clone(),
        entrypoint,
        placeholder_factory,
        placeholder_weth,
    )
    .await?;
    let adapter_address =
        deploy_fee_adapter(provider.clone(), paymaster_address, pool.address).await?;

    pool.paymaster_address = Some(paymaster_address);
    pool.adapter_address = Some(adapter_address);

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
    let alto = AltoBuilder::new(
        anvil.endpoint_url().to_string(),
        entrypoint,
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
