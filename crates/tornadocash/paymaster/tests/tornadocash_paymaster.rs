use alloy::{
    node_bindings::Anvil,
    primitives::{Address, address},
    providers::{Provider, ProviderBuilder},
    signers::local::PrivateKeySigner,
};
use kohaku_fork_kit::{
    alto::AltoBuilder,
    artifacts::tornado_artifacts,
    entry_point::deploy_entry_point,
    paymaster::{allow_instance, deploy_paymaster},
    pool::deploy_pool,
    simple_account::deploy_simple_account,
};
use kohaku_tornadocash::{
    Asset, Deposit, Pool, Withdrawal,
    merkle_tree::{MerkleTree, MerkleTreeExt},
};
use kohaku_tornadocash_paymaster::TornadoPaymasterExt;
use kohaku_tornadocash_provider::TornadoProviderExt;
use kohaku_tornadocash_syncer::{Syncer, rpc::RpcSyncer};
use kohaku_userop_kit::{
    bundler::BundlerApi,
    smart_account::{SIMPLE_7702_ACCOUNT_IMPLEMENTATION_V08, Simple7702Account},
};
use tracing::info;

const ALTO_EXECUTOR_PK: &str = "0x4a3a02862ddcb260ed52d40ef03f8e3d78fa3d174b0ef333afdf1ffb4a648cd5";
const ALTO_UTILITY_PK: &str = "0xdd4b2564c83ff7de602c39ffda1146055dc1814b07c083d7971722384f1f01a6";

const PLACEHOLDER_FACTORY: Address = address!("0x0000000000000000000000000000000000000011");
const PLACEHOLDER_WETH: Address = address!("0x0000000000000000000000000000000000000022");

#[tokio::test]
#[ignore = "run with `cargo test --release -- --ignored`"]
async fn test_tornadocash_paymaster() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init()
        .ok();

    let anvil = Anvil::new().try_spawn()?;
    let wallet = anvil.wallet().expect("anvil provides a dev wallet");
    let provider = ProviderBuilder::new()
        .wallet(wallet)
        .connect_http(anvil.endpoint_url())
        .erased();
    let chain_id = provider.get_chain_id().await?;

    let entrypoint = deploy_entry_point(&provider).await?;
    deploy_simple_account(&provider).await?;

    let mut pool = Pool::new(
        provider.get_chain_id().await?,
        deploy_pool(provider.clone(), 10_u128.pow(17)).await?,
        Asset::ETH,
        10_u128.pow(17),
        0,
        None,
    );
    let paymaster_address = deploy_paymaster(
        provider.clone(),
        entrypoint,
        PLACEHOLDER_FACTORY,
        PLACEHOLDER_WETH,
    )
    .await?;
    allow_instance(provider.clone(), paymaster_address, pool.address).await?;
    pool.paymaster = Some(paymaster_address);

    // Deposit a note
    info!("Depositing into pool");
    let deposit = Deposit::new(&pool, rand::random());
    let note = deposit.note.clone();
    provider
        .send_transaction(deposit.call().into())
        .await?
        .watch()
        .await?;

    // Sync a Merkle tree against the provider
    let syncer = RpcSyncer::new(provider.clone());
    let mut tree = MerkleTree::new();
    let snapshot = syncer.sync(&pool, ..).await?;
    tree.splice_events(&snapshot)?;

    info!("Starting local alto bundler");
    let alto = AltoBuilder::new(
        anvil.endpoint(),
        entrypoint,
        ALTO_EXECUTOR_PK,
        ALTO_UTILITY_PK,
    )
    .prefund(&provider)
    .await?
    .spawn()
    .await?;

    // Sponsor a UserOp with the note
    info!("Withdrawing from pool with tornadocash paymaster");
    let owner = PrivateKeySigner::random();
    let smart_account = Simple7702Account::new(
        provider.clone(),
        owner.address(),
        SIMPLE_7702_ACCOUNT_IMPLEMENTATION_V08,
        chain_id,
    );

    let withdrawal = Withdrawal::new(&pool, note.clone(), owner.address());
    let withdrawal_merkle_proof = tree.note_proof(&withdrawal.note)?;
    let (userop, _fee) = alto
        .prepare_tornado_withdrawal(
            &smart_account,
            withdrawal,
            &withdrawal_merkle_proof,
            &provider,
            tornado_artifacts(),
            &mut rand::rng(),
        )
        .await?;
    let pending = alto
        .send_user_operation(&userop.sign(&owner).await?)
        .await?;
    let userop_receipt = pending.get_receipt().await?;
    info!("Userop receipt: {userop_receipt:?}");

    assert!(userop_receipt.success, "userop should succeed");
    assert!(
        provider.is_spent(&pool, note.nullifier_hash()).await?,
        "note should be spent"
    );

    Ok(())
}
