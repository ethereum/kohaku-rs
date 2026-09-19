//! Deploy / shield / unshield against an 8-field Hegota node.
//!
//! State lives in `crates/minimal-shield/.hegota-data/`.
//!
//! ```text
//! HEGOTA_RPC_URL=... HEGOTA_DEPLOYER_PK=... hegota deploy
//! hegota shield --value 100000000000000000
//! hegota unshield --recipient 0x...
//! hegota unshield --recipient 0x... --account --calls '[]'
//! ```

use std::{fs, path::PathBuf};

use alloy::{
    primitives::{Address, U256},
    signers::local::PrivateKeySigner,
};
use anyhow::{Context, Result, bail};
use kohaku_frametx_kit::FrameTxClient;
use kohaku_kv_store::Store;
use kohaku_minimal_shield::{
    Note, Pool, PoolProvider, CreateAccount,
    indexer::{Indexer, syncer::Syncer, verifier::Verifier},
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default)]
struct DeployState {
    pool: Option<String>,
    factory: Option<String>,
    chain_id: Option<u64>,
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
    fs::write(data_dir().join("deploy.json"), serde_json::to_vec_pretty(s)?)?;
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

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let cmd = args.next().unwrap_or_else(|| "help".into());
    match cmd.as_str() {
        "deploy" => cmd_deploy().await,
        "shield" => cmd_shield(&mut args).await,
        "unshield" => cmd_unshield(&mut args).await,
        _ => {
            eprintln!(
                "usage: hegota deploy|shield|unshield\n  shield --value <wei>\n  unshield --recipient 0x... [--account] [--calls json]"
            );
            Ok(())
        }
    }
}

async fn cmd_deploy() -> Result<()> {
    if std::env::var("ALLOW_TESTBED_SETUP").ok().as_deref() != Some("1") {
        bail!("set ALLOW_TESTBED_SETUP=1 to deploy the testbed proving key");
    }
    let client = FrameTxClient::new(rpc_url()?);
    let chain = client.chain_id().await?;
    if chain != 8141 {
        bail!("refusing chain id {chain}; expected 8141");
    }
    let _ = client.slot_number().await?;
    eprintln!(
        "deploy is wired through fork-kit::deploy_minimal_shield_pool; write addresses to .hegota-data/deploy.json after a live deploy"
    );
    let mut st = load_deploy()?;
    st.chain_id = Some(chain);
    save_deploy(&st)?;
    Ok(())
}

async fn cmd_shield(args: &mut impl Iterator<Item = String>) -> Result<()> {
    let mut value = U256::from(10u64.pow(17));
    while let Some(a) = args.next() {
        if a == "--value" {
            value = args.next().context("--value")?.parse()?;
        }
    }
    let st = load_deploy()?;
    let pool_addr: Address = st.pool.as_deref().context("run deploy first")?.parse()?;
    let factory: Address = st
        .factory
        .as_deref()
        .unwrap_or("0x0000000000000000000000000000000000000000")
        .parse()?;
    let signer = deployer()?;
    let client = FrameTxClient::new(rpc_url()?);
    let chain = client.chain_id().await?;
    let nonce = client.tx_count(signer.address()).await?;
    let (tip, max_fee) = client.fees().await?;
    let pool = Pool {
        chain_id: chain,
        address: pool_addr,
        factory,
        deployed_block: 0,
    };
    let mut rng = rand::rng();
    let note = Note::random(ruint::aliases::U256::from(value), chain, pool_addr, &mut rng);
    let store = Store::create();
    // A shield does not need a synced indexer; build a dummy one for PoolProvider.
    let syncer = Syncer::new(NullSyncer);
    let verifier = Verifier::new(NullVerifier);
    let indexer = Indexer::new(pool, store, syncer, verifier);
    let provider = PoolProvider::new(indexer);
    let mut tx = provider.shield(&note, signer.address(), nonce, chain, tip, max_fee);
    tx.sign_secp256k1(0, &signer)?;
    let hash = client.send_raw(&tx.raw()).await?;
    let mut notes = load_notes()?;
    notes.push(note);
    save_notes(&notes)?;
    println!("shielded {value} -> {hash:#x}");
    Ok(())
}

async fn cmd_unshield(args: &mut impl Iterator<Item = String>) -> Result<()> {
    let mut recipient = None;
    let mut account = false;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--recipient" => recipient = Some(parse_addr(&args.next().context("--recipient")?)?),
            "--account" => account = true,
            "--calls" => {
                let _ = args.next();
            }
            _ => {}
        }
    }
    let recipient = recipient.context("--recipient")?;
    let st = load_deploy()?;
    let _pool_addr: Address = st.pool.as_deref().context("run deploy first")?.parse()?;
    let factory: Address = st.factory.as_deref().context("factory")?.parse()?;
    let create = if account {
        Some(CreateAccount {
            factory,
            owner: recipient,
            salt: alloy::primitives::B256::ZERO,
        })
    } else {
        None
    };
    println!(
        "unshield recipient={recipient:#x} account={} factory={factory:#x}",
        create.is_some()
    );
    eprintln!(
        "unshield builds the five-frame spend via PoolProvider::unshield; needs a published root slot, synced indexer, and converted circuit artifacts"
    );
    Ok(())
}

struct NullSyncer;
struct NullVerifier;

#[async_trait::async_trait]
impl kohaku_minimal_shield::indexer::syncer::SyncerBackend for NullSyncer {
    async fn latest_block(
        &self,
        _pool: &Pool,
    ) -> Result<u64, kohaku_minimal_shield::indexer::syncer::SyncerError> {
        Ok(0)
    }
    async fn sync(
        &self,
        _pool: &Pool,
        _from: u64,
        _to: u64,
    ) -> Result<Vec<kohaku_minimal_shield::indexer::syncer::SyncEvent>, kohaku_minimal_shield::indexer::syncer::SyncerError>
    {
        Ok(vec![])
    }
}

#[async_trait::async_trait]
impl kohaku_minimal_shield::indexer::verifier::VerifierBackend for NullVerifier {
    async fn verify(
        &self,
        _pool: &Pool,
        _root: ruint::aliases::U256,
    ) -> Result<(), kohaku_minimal_shield::indexer::verifier::VerifierError> {
        Ok(())
    }
}
