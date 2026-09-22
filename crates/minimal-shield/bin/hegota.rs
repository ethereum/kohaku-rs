//! Deploy / shield / unshield against an 8-field Hegota node.
//!
//! State lives in `crates/minimal-shield/.hegota-data/`.
//!
//! ```text
//! HEGOTA_RPC_URL=... HEGOTA_DEPLOYER_PK=... ALLOW_TESTBED_SETUP=1 hegota deploy
//! hegota shield --value 500000000000000000
//! hegota unshield --recipient 0x...
//! hegota unshield-4337 --to 0x... --amount 10000000000000000
//! ```

use std::{fs, path::PathBuf, str::FromStr};

use alloy::{
    network::EthereumWallet,
    primitives::{Address, B256, Bytes, U256 as AlloyU256, aliases::U192},
    providers::{Provider, ProviderBuilder},
    signers::local::PrivateKeySigner,
    sol_types::SolCall,
};
use anyhow::{Context, Result, bail};
use kohaku_fork_kit::minimal_shield::deploy_minimal_shield_pool;
use kohaku_frametx_kit::{
    FrameTxClient, TAIL_4337_FRAME_GAS, TAIL_4337_FRAME_STATE_GAS, recent_root_window_error,
};
use kohaku_kv_store::{Store, file::FileStore};
use kohaku_minimal_shield::{
    Note, Pool, PoolProvider, TailCall,
    abis::{
        EntryPoint4337::{self, PackedUserOperation as HandleOp},
        Multicall3::{self, Call3},
        ShieldedPool, SimpleAccount, SimpleAccountFactory,
    },
    indexer::{Indexer, rpc::RpcSyncer, syncer::Syncer, verifier::Verifier},
};
use kohaku_userop_kit::{
    builder::UserOperationBuilder, entry_point::entry_point_domain,
    user_operation::UserOperationGasEstimate,
};
use ruint::aliases::U256;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default)]
struct DeployState {
    pool: Option<String>,
    logic: Option<String>,
    verifier: Option<String>,
    poseidon_t3: Option<String>,
    poseidon_t4: Option<String>,
    multicall3: Option<String>,
    entry_point: Option<String>,
    simple_account_factory: Option<String>,
    simple_account_impl: Option<String>,
    chain_id: Option<u64>,
    deployed_block: Option<u64>,
    root_slot: Option<u64>,
}

#[derive(Serialize, Deserialize)]
struct NoteFile {
    notes: Vec<Note>,
}

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".hegota-data")
}

fn load_deploy() -> Result<DeployState> {
    let p = data_dir().join("deploy.json");
    if !p.exists() {
        return Ok(DeployState::default());
    }
    Ok(serde_json::from_slice(&fs::read(p)?)?)
}

fn save_deploy(s: &DeployState) -> Result<()> {
    fs::create_dir_all(data_dir())?;
    fs::write(
        data_dir().join("deploy.json"),
        serde_json::to_vec_pretty(s)?,
    )?;
    Ok(())
}

fn load_notes() -> Result<Vec<Note>> {
    let p = data_dir().join("notes.json");
    if !p.exists() {
        return Ok(Vec::new());
    }
    let f: NoteFile = serde_json::from_slice(&fs::read(p)?)?;
    Ok(f.notes)
}

fn save_notes(notes: &[Note]) -> Result<()> {
    fs::create_dir_all(data_dir())?;
    fs::write(
        data_dir().join("notes.json"),
        serde_json::to_vec_pretty(&NoteFile {
            notes: notes.to_vec(),
        })?,
    )?;
    Ok(())
}

fn rpc_url() -> Result<reqwest::Url> {
    std::env::var("HEGOTA_RPC_URL")
        .context("HEGOTA_RPC_URL")?
        .parse()
        .context("rpc url")
}

fn deployer() -> Result<PrivateKeySigner> {
    let pk = std::env::var("HEGOTA_DEPLOYER_PK").context("HEGOTA_DEPLOYER_PK")?;
    pk.parse().context("deployer key")
}

fn parse_addr(s: &str) -> Result<Address> {
    s.parse().context("address")
}

fn require_addr(opt: Option<&str>, name: &str) -> Result<Address> {
    parse_addr(opt.with_context(|| format!("run deploy first ({name})"))?)
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let cmd = args.next().unwrap_or_else(|| "help".into());
    match cmd.as_str() {
        "deploy" => cmd_deploy().await,
        "shield" => cmd_shield(&mut args).await,
        "unshield" => cmd_unshield(&mut args).await,
        "unshield-4337" => cmd_unshield_4337(&mut args).await,
        _ => {
            eprintln!(
                "usage: hegota deploy|shield|unshield|unshield-4337\n  \
                 shield --value <wei>\n  \
                 unshield --recipient 0x...\n  \
                 unshield-4337 [--to 0x...] [--amount <wei>] [--owner-pk 0x...] [--salt <u256>]"
            );
            Ok(())
        }
    }
}

async fn cmd_deploy() -> Result<()> {
    if std::env::var("ALLOW_TESTBED_SETUP").ok().as_deref() != Some("1") {
        bail!("set ALLOW_TESTBED_SETUP=1 to deploy the testbed proving key");
    }
    let signer = deployer()?;
    let wallet = EthereumWallet::from(signer.clone());
    let provider = ProviderBuilder::new()
        .wallet(wallet)
        .connect_http(rpc_url()?);
    let client = FrameTxClient::new(rpc_url()?);
    let d = deploy_minimal_shield_pool(provider, &client).await?;
    let st = DeployState {
        pool: Some(format!("{:#x}", d.pool)),
        logic: Some(format!("{:#x}", d.logic)),
        verifier: Some(format!("{:#x}", d.verifier)),
        poseidon_t3: Some(format!("{:#x}", d.poseidon_t3)),
        poseidon_t4: Some(format!("{:#x}", d.poseidon_t4)),
        multicall3: Some(format!("{:#x}", d.multicall3)),
        entry_point: Some(format!("{:#x}", d.entry_point)),
        simple_account_factory: Some(format!("{:#x}", d.simple_account_factory)),
        simple_account_impl: Some(format!("{:#x}", d.simple_account_impl)),
        chain_id: Some(client.chain_id().await?),
        deployed_block: Some(d.deployed_block),
        root_slot: None,
    };
    save_deploy(&st)?;
    if data_dir().join("indexer.redb").exists() {
        fs::remove_file(data_dir().join("indexer.redb"))?;
    }
    println!("pool {:#x}", d.pool);
    println!("multicall3 {:#x}", d.multicall3);
    println!("entryPoint {:#x}", d.entry_point);
    println!("simpleAccountFactory {:#x}", d.simple_account_factory);
    println!("wrote {}", data_dir().join("deploy.json").display());
    Ok(())
}

async fn cmd_shield(args: &mut impl Iterator<Item = String>) -> Result<()> {
    let mut value = U256::from(10u64.pow(17));
    while let Some(a) = args.next() {
        if a == "--value" {
            value = args.next().context("--value")?.parse()?;
        }
    }
    let mut st = load_deploy()?;
    let pool_addr = require_addr(st.pool.as_deref(), "pool")?;
    let factory = st
        .simple_account_factory
        .as_deref()
        .map(parse_addr)
        .transpose()?
        .unwrap_or(Address::ZERO);
    let signer = deployer()?;
    let client = FrameTxClient::new(rpc_url()?);
    let chain = client.chain_id().await?;
    let nonce = client.tx_count(signer.address()).await?;
    let (tip, max_fee) = client.fees().await?;
    let provider = http_provider()?;
    let pool = Pool {
        chain_id: chain,
        address: pool_addr,
        factory,
        deployed_block: st.deployed_block.unwrap_or(0),
    };
    let mut rng = rand::rng();
    let note = Note::random(value, chain, pool_addr, &mut rng);
    let msp = pool_provider(pool, provider.clone())?;
    let mut tx = msp.shield(&note, signer.address(), nonce, chain, tip, max_fee);
    tx.sign_secp256k1(0, &signer)?;
    let hash = client.send_raw(&tx.raw()).await?;
    wait_inclusion(&client, hash).await?;
    msp.indexer.sync().await?;
    let epoch = ShieldedPool::new(pool_addr, &provider)
        .currentEpoch()
        .call()
        .await?;
    let slot = publish_root(&provider, &client, pool_addr, epoch).await?;
    st.root_slot = Some(slot);
    save_deploy(&st)?;
    let mut notes = load_notes()?;
    notes.push(note);
    save_notes(&notes)?;
    println!("shielded {value} -> {hash:#x}; published epoch {epoch} at slot {slot}");
    Ok(())
}

async fn cmd_unshield(args: &mut impl Iterator<Item = String>) -> Result<()> {
    let mut recipient = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--recipient" => recipient = Some(parse_addr(&args.next().context("--recipient")?)?),
            "--account" | "--calls" => {
                bail!("FrameAccount path removed; use unshield-4337 for a smart-account tail");
            }
            other => bail!("unknown flag {other}"),
        }
    }
    let recipient = recipient.context("--recipient")?;
    let before = balance_of(recipient).await?;
    let (hash, public_amount) = spend_with_tail(recipient, |msp| msp.claim_tail(recipient)).await?;
    let after = balance_of(recipient).await?;
    println!(
        "unshield {hash:#x} publicAmount={public_amount} recipient={recipient:#x} {before} -> {after} wei"
    );
    Ok(())
}

async fn cmd_unshield_4337(args: &mut impl Iterator<Item = String>) -> Result<()> {
    let mut dest: Option<Address> = None;
    let mut amount: Option<AlloyU256> = None;
    let mut owner_pk: Option<PrivateKeySigner> = None;
    let mut salt = AlloyU256::ZERO;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--to" => dest = Some(parse_addr(&args.next().context("--to")?)?),
            "--amount" => amount = Some(args.next().context("--amount")?.parse()?),
            "--owner-pk" => {
                owner_pk = Some(
                    args.next()
                        .context("--owner-pk")?
                        .parse()
                        .context("owner pk")?,
                );
            }
            "--salt" => salt = args.next().context("--salt")?.parse()?,
            other => bail!("unknown flag {other}"),
        }
    }
    let dest = dest.unwrap_or_else(|| PrivateKeySigner::random().address());
    let owner = match owner_pk {
        Some(s) => s,
        None => deployer()?,
    };
    let st = load_deploy()?;
    let factory_addr = require_addr(
        st.simple_account_factory.as_deref(),
        "simple_account_factory",
    )?;
    let entry_point = require_addr(st.entry_point.as_deref(), "entry_point")?;
    let multicall3 = require_addr(st.multicall3.as_deref(), "multicall3")?;
    let pool_addr = require_addr(st.pool.as_deref(), "pool")?;
    let send_amount = amount.unwrap_or(AlloyU256::from(10_000_000_000_000_000u64));
    if send_amount.is_zero() {
        bail!("--amount must be positive");
    }
    let provider = http_provider()?;
    let account = SimpleAccountFactory::new(factory_addr, &provider)
        .getAddress(owner.address(), salt)
        .call()
        .await?;
    let tail = zero_price_userop_tail(
        &provider,
        &owner,
        factory_addr,
        entry_point,
        multicall3,
        pool_addr,
        account,
        dest,
        send_amount,
        salt,
    )
    .await?;
    let before_dest = balance_of(dest).await?;
    let before_account = balance_of(account).await?;
    let (hash, public_amount) = spend_with_tail(account, |_| tail).await?;
    let after_dest = balance_of(dest).await?;
    let after_account = balance_of(account).await?;
    println!(
        "unshield-4337 {hash:#x} account={account:#x} dest={dest:#x} publicAmount={public_amount}"
    );
    println!("  dest {before_dest} -> {after_dest} wei (wanted {send_amount})");
    println!("  account {before_account} -> {after_account} wei");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn zero_price_userop_tail(
    provider: &impl Provider,
    owner: &PrivateKeySigner,
    factory_addr: Address,
    entry_point: Address,
    multicall3: Address,
    pool_addr: Address,
    account: Address,
    dest: Address,
    send_amount: AlloyU256,
    salt: AlloyU256,
) -> Result<TailCall> {
    let code = provider.get_code_at(account).await?;
    let factory_data = Bytes::from(
        SimpleAccountFactory::createAccountCall {
            owner: owner.address(),
            salt,
        }
        .abi_encode(),
    );
    let chain = FrameTxClient::new(rpc_url()?).chain_id().await?;
    let domain = entry_point_domain(chain, entry_point);
    let nonce = EntryPoint4337::new(entry_point, provider)
        .getNonce(account, U192::ZERO)
        .call()
        .await?;
    let execute = Bytes::from(
        SimpleAccount::executeCall {
            dest,
            value: send_amount,
            func: Bytes::new(),
        }
        .abi_encode(),
    );
    let mut builder = UserOperationBuilder::<()>::new(account, entry_point, domain)
        .with_nonce(nonce)
        .with_calldata(execute)
        .with_gas(UserOperationGasEstimate {
            pre_verification_gas: 0,
            verification_gas_limit: 1_000_000,
            call_gas_limit: 250_000,
            paymaster_verification_gas_limit: None,
            paymaster_post_op_gas_limit: None,
            max_fee_per_gas: 0,
            max_priority_fee_per_gas: 0,
        });
    if code.is_empty() {
        builder = builder.with_factory(factory_addr, factory_data);
    }
    let signed = builder.build().sign(owner).await?;
    let packed = kohaku_userop_kit::abis::entry_point::EntryPoint::PackedUserOperation::from(
        &signed.user_op,
    );
    let op = HandleOp {
        sender: packed.sender,
        nonce: packed.nonce,
        initCode: packed.initCode,
        callData: packed.callData,
        accountGasLimits: packed.accountGasLimits,
        preVerificationGas: packed.preVerificationGas,
        gasFees: packed.gasFees,
        paymasterAndData: packed.paymasterAndData,
        signature: ecdsa_v27(signed.user_op.signature.clone()),
    };
    let claim = Bytes::from(ShieldedPool::claimWithdrawalCall { who: account }.abi_encode());
    let handle = Bytes::from(
        EntryPoint4337::handleOpsCall {
            ops: vec![op],
            beneficiary: account,
        }
        .abi_encode(),
    );
    Ok(TailCall {
        target: multicall3,
        data: Bytes::from(
            Multicall3::aggregate3Call {
                calls: vec![
                    Call3 {
                        target: pool_addr,
                        allowFailure: false,
                        callData: claim,
                    },
                    Call3 {
                        target: entry_point,
                        allowFailure: false,
                        callData: handle,
                    },
                ],
            }
            .abi_encode(),
        ),
        execution_gas: TAIL_4337_FRAME_GAS,
        state_gas: TAIL_4337_FRAME_STATE_GAS,
    })
}

async fn spend_with_tail(
    recipient: Address,
    tail: impl FnOnce(&PoolProvider) -> TailCall,
) -> Result<(B256, U256)> {
    let st = load_deploy()?;
    let pool_addr = require_addr(st.pool.as_deref(), "pool")?;
    let factory = st
        .simple_account_factory
        .as_deref()
        .map(parse_addr)
        .transpose()?
        .unwrap_or(Address::ZERO);
    let note = last_note()?;
    let signer = deployer()?;
    let client = FrameTxClient::new(rpc_url()?);
    let chain = client.chain_id().await?;
    let (tip, max_fee) = client.fees().await?;
    let provider = http_provider()?;
    let pool = Pool {
        chain_id: chain,
        address: pool_addr,
        factory,
        deployed_block: st.deployed_block.unwrap_or(0),
    };
    let msp = pool_provider(pool, provider.clone())?;
    msp.indexer.sync().await?;
    let epoch = ShieldedPool::new(pool_addr, &provider)
        .currentEpoch()
        .call()
        .await?;
    let mut st = st;
    let root_slot = if let Some(s) = st.root_slot {
        s
    } else {
        let s = publish_root(&provider, &client, pool_addr, epoch).await?;
        st.root_slot = Some(s);
        save_deploy(&st)?;
        s
    };
    if let Some(err) = recent_root_window_error(root_slot, client.slot_number().await?) {
        bail!("{err}");
    }
    let dummy = dummy_for(&note);
    let tail = tail(&msp);
    let result = msp
        .unshield(
            &note,
            &dummy,
            recipient,
            Some(tail),
            None,
            &signer,
            root_slot,
            epoch,
            chain,
            tip,
            max_fee,
        )
        .await?;
    if let Err(e) = client.gate_spend(&result.tx).await {
        eprintln!("simulate: {e}; sending anyway");
    }
    let hash = client.send_raw(&result.tx.raw()).await?;
    wait_inclusion(&client, hash).await?;
    msp.indexer.sync().await?;
    let mut notes = load_notes()?;
    notes.retain(|n| n != &note);
    save_notes(&notes)?;
    Ok((hash, result.public_amount))
}

fn dummy_for(note: &Note) -> Note {
    let mut rng = rand::rng();
    Note::random(U256::ZERO, note.chain_id, note.pool, &mut rng)
}

fn last_note() -> Result<Note> {
    load_notes()?
        .pop()
        .context("no shielded notes; run shield first")
}

fn pool_provider(pool: Pool, provider: impl Provider + Clone + 'static) -> Result<PoolProvider> {
    fs::create_dir_all(data_dir())?;
    let store = Store::new(FileStore::open(data_dir().join("indexer.redb"))?);
    let rpc = RpcSyncer::new(provider);
    let indexer = Indexer::new(pool, store, Syncer::new(rpc.clone()), Verifier::new(rpc));
    Ok(PoolProvider::new(indexer))
}

fn http_provider() -> Result<impl Provider + Clone + 'static> {
    let signer = deployer()?;
    let wallet = EthereumWallet::from(signer);
    Ok(ProviderBuilder::new()
        .wallet(wallet)
        .connect_http(rpc_url()?))
}

async fn publish_root<P: Provider>(
    provider: &P,
    client: &FrameTxClient,
    pool: Address,
    epoch: u64,
) -> Result<u64> {
    let pending = ShieldedPool::new(pool, provider)
        .publishEpochRoot(epoch)
        .send()
        .await?;
    let hash = *pending.tx_hash();
    let receipt = wait_inclusion(client, hash).await?;
    let block_hash = receipt
        .get("blockHash")
        .and_then(serde_json::Value::as_str)
        .context("publish receipt missing blockHash")?;
    let block_hash = B256::from_str(block_hash)?;
    let block_number = receipt
        .get("blockNumber")
        .and_then(|v| {
            v.as_str()
                .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
        })
        .context("publish receipt missing blockNumber")?;
    loop {
        if client.block_number().await? >= block_number.saturating_add(2) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    client.slot_number_of(block_hash).await.map_err(Into::into)
}

async fn wait_inclusion(client: &FrameTxClient, hash: B256) -> Result<serde_json::Value> {
    let receipt = client.wait_receipt(hash, 120).await?;
    let status = receipt
        .get("status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("0x0");
    if status != "0x1" {
        bail!("tx {hash:#x} reverted (status {status})");
    }
    Ok(receipt)
}

async fn balance_of(addr: Address) -> Result<AlloyU256> {
    Ok(http_provider()?.get_balance(addr).await?)
}

fn ecdsa_v27(sig: Bytes) -> Bytes {
    let mut raw = sig.to_vec();
    if raw.len() == 65 && raw[64] < 27 {
        raw[64] += 27;
    }
    raw.into()
}
