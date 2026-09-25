//! Deploy MSP HEAD + FrameAccount infra on an 8-field Hegota node.

use std::{
    path::{Path, PathBuf},
    process::Stdio,
};

use alloy::{primitives::Address, providers::Provider};
use anyhow::{Context, Result, bail};
use kohaku_frametx_kit::{FrameTxClient, HEGOTA_CHAIN_ID};
use serde::Deserialize;
use tokio::process::Command;

/// Addresses produced by [`deploy_minimal_shield_pool`].
#[derive(Debug, Clone)]
pub struct MinimalShieldDeployment {
    pub poseidon_t3: Address,
    pub poseidon_t4: Address,
    pub verifier: Address,
    pub logic: Address,
    pub pool: Address,
    pub multicall3: Address,
    pub frame_account_factory: Address,
    pub deployed_block: u64,
}

#[derive(Debug, Deserialize)]
struct MspDeployConfig {
    pool: String,
    verifier: String,
    #[serde(rename = "poseidonT3")]
    poseidon_t3: String,
    #[serde(rename = "poseidonT4")]
    poseidon_t4: String,
    logic: String,
    profile: Option<String>,
}

/// Deploy only [`FrameAccountFactory`] via `forge create`. Reuses an existing
/// Multicall3; does not touch the MSP pool.
///
/// # Errors
/// Returns if `FRAME_ACCT_ROOT` / the sibling checkout is missing or forge fails.
pub async fn deploy_frame_account_factory() -> Result<Address> {
    let rpc_url = std::env::var("HEGOTA_RPC_URL").context("HEGOTA_RPC_URL")?;
    let deployer_pk = std::env::var("HEGOTA_DEPLOYER_PK").context("HEGOTA_DEPLOYER_PK")?;
    let frame_root = sibling_repo("FRAME_ACCT_ROOT", "frame-privacy-acct")?;
    let addr = forge_create(
        &frame_root,
        &rpc_url,
        &deployer_pk,
        "src/FrameAccountFactory.sol:FrameAccountFactory",
        CREATE_GAS,
        &[],
        &[],
    )
    .await?;
    eprintln!("  FrameAccountFactory {addr:#x}");
    Ok(addr)
}

/// Deploy the MSP dispatcher pool (`SPEND=0`) and FrameAccount factory + Multicall3.
///
/// Reads `MSP_ROOT` (default `../minimal-shielded-pool` next to this repo) and
/// `FRAME_ACCT_ROOT` (default `../frame-privacy-acct`). Uses `HEGOTA_RPC_URL` /
/// `HEGOTA_DEPLOYER_PK` for the child processes.
///
/// # Errors
/// Returns if the RPC is not Hegota, the sibling repos are missing, or a
/// subprocess fails.
pub async fn deploy_minimal_shield_pool<P: Provider>(
    provider: P,
    rpc: &FrameTxClient,
) -> Result<MinimalShieldDeployment> {
    if std::env::var("ALLOW_TESTBED_SETUP").ok().as_deref() != Some("1") {
        bail!("set ALLOW_TESTBED_SETUP=1 to deploy the disposable testbed proving key");
    }
    let chain = provider.get_chain_id().await?;
    if chain != HEGOTA_CHAIN_ID {
        bail!("refusing chain id {chain}; expected {HEGOTA_CHAIN_ID}");
    }
    let _ = rpc.slot_number().await.context("EIP-7843 slotNumber")?;

    let rpc_url = std::env::var("HEGOTA_RPC_URL").context("HEGOTA_RPC_URL")?;
    let deployer_pk = std::env::var("HEGOTA_DEPLOYER_PK").context("HEGOTA_DEPLOYER_PK")?;
    let msp_root = sibling_repo("MSP_ROOT", "minimal-shielded-pool")?;
    let frame_root = sibling_repo("FRAME_ACCT_ROOT", "frame-privacy-acct")?;

    let msp = if std::env::var("SKIP_MSP").ok().as_deref() == Some("1") {
        eprintln!("SKIP_MSP=1: reading existing deploy_config.json");
        read_msp_config(&msp_root)?
    } else {
        deploy_msp(&msp_root, &rpc_url, &deployer_pk).await?
    };
    let accounts = deploy_account_infra(&frame_root, &rpc_url, &deployer_pk).await?;
    let deployed_block = provider.get_block_number().await?;

    Ok(MinimalShieldDeployment {
        poseidon_t3: msp.poseidon_t3,
        poseidon_t4: msp.poseidon_t4,
        verifier: msp.verifier,
        logic: msp.logic,
        pool: msp.pool,
        multicall3: accounts.multicall3,
        frame_account_factory: accounts.frame_account_factory,
        deployed_block,
    })
}

struct MspAddrs {
    pool: Address,
    verifier: Address,
    poseidon_t3: Address,
    poseidon_t4: Address,
    logic: Address,
}

struct AccountAddrs {
    multicall3: Address,
    frame_account_factory: Address,
}

async fn deploy_msp(msp_root: &Path, rpc_url: &str, deployer_pk: &str) -> Result<MspAddrs> {
    let script = msp_root.join("devnet/run_live_dispatcher.sh");
    if !script.is_file() {
        bail!("MSP deploy script missing at {}", script.display());
    }
    let status = Command::new("bash")
        .arg(&script)
        .current_dir(msp_root.join("devnet"))
        .env("RPC_URL", rpc_url)
        .env("DEPLOYER_PK", deployer_pk)
        .env("ALLOW_TESTBED_SETUP", "1")
        .env("SPEND", "0")
        .stdin(Stdio::null())
        .status()
        .await
        .context("run_live_dispatcher.sh")?;
    if !status.success() {
        bail!("run_live_dispatcher.sh failed with {status}");
    }
    read_msp_config(msp_root)
}

fn read_msp_config(msp_root: &Path) -> Result<MspAddrs> {
    let cfg_path = msp_root.join("devnet/deploy_config.json");
    let cfg: MspDeployConfig = serde_json::from_slice(
        &std::fs::read(&cfg_path).with_context(|| format!("read {}", cfg_path.display()))?,
    )
    .context("parse MSP deploy_config.json")?;
    if cfg.profile.as_deref() != Some("position-notes-v2") {
        bail!(
            "MSP deploy_config profile is {:?}; expected position-notes-v2",
            cfg.profile
        );
    }
    Ok(MspAddrs {
        pool: parse_addr(&cfg.pool, "pool")?,
        verifier: parse_addr(&cfg.verifier, "verifier")?,
        poseidon_t3: parse_addr(&cfg.poseidon_t3, "poseidonT3")?,
        poseidon_t4: parse_addr(&cfg.poseidon_t4, "poseidonT4")?,
        logic: parse_addr(&cfg.logic, "logic")?,
    })
}

const HEGOTA_GAS_PRICE: &str = "3000000000";
const HEGOTA_PRIORITY_FEE: &str = "1000000000";
/// Explicit create gas. forge script estimates ~1M and OOGs on Hegotá (EIP-8037
/// state). Multicall3 is ~5.3M; 5M is too tight. Small creates use 12M.
const CREATE_GAS: &str = "12000000";

async fn deploy_account_infra(
    frame_root: &Path,
    rpc_url: &str,
    deployer_pk: &str,
) -> Result<AccountAddrs> {
    let multicall3 = forge_create(
        frame_root,
        rpc_url,
        deployer_pk,
        "lib/multicall/src/Multicall3.sol:Multicall3",
        CREATE_GAS,
        &[],
        &[],
    )
    .await?;
    eprintln!("  Multicall3 {multicall3:#x}");

    let frame_account_factory = forge_create(
        frame_root,
        rpc_url,
        deployer_pk,
        "src/FrameAccountFactory.sol:FrameAccountFactory",
        CREATE_GAS,
        &[],
        &[],
    )
    .await?;
    eprintln!("  FrameAccountFactory {frame_account_factory:#x}");

    Ok(AccountAddrs {
        multicall3,
        frame_account_factory,
    })
}

async fn forge_create(
    frame_root: &Path,
    rpc_url: &str,
    deployer_pk: &str,
    contract: &str,
    gas: &str,
    constructor_args: &[&str],
    extra: &[&str],
) -> Result<Address> {
    let mut cmd = Command::new("forge");
    cmd.args([
        "create",
        contract,
        "--rpc-url",
        rpc_url,
        "--private-key",
        deployer_pk,
        "--broadcast",
        "--gas-price",
        HEGOTA_GAS_PRICE,
        "--priority-gas-price",
        HEGOTA_PRIORITY_FEE,
        "--gas-limit",
        gas,
    ])
    .args(extra)
    .current_dir(frame_root);
    if !constructor_args.is_empty() {
        cmd.arg("--constructor-args").args(constructor_args);
    }
    eprintln!("==> forge create {contract} --gas-limit {gas}");
    let output = cmd
        .output()
        .await
        .with_context(|| format!("forge create {contract}"))?;
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    eprint!("{combined}");
    if !output.status.success() {
        bail!("forge create {contract} failed with {}", output.status);
    }
    parse_deployed_to(&combined, contract)
}

fn parse_deployed_to(output: &str, label: &str) -> Result<Address> {
    for line in output.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Deployed to:") {
            return parse_addr(rest.trim().split_whitespace().next().unwrap_or(""), label);
        }
    }
    bail!("did not find `Deployed to:` in forge create {label} output")
}

fn sibling_repo(env: &str, name: &str) -> Result<PathBuf> {
    if let Ok(p) = std::env::var(env) {
        let path = PathBuf::from(p);
        if path.is_dir() {
            return Ok(path);
        }
        bail!("{env}={} is not a directory", path.display());
    }
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = crate_dir.join("../..").join("..").join(name);
    let path = path.canonicalize().unwrap_or(path);
    if path.is_dir() {
        return Ok(path);
    }
    bail!(
        "set {env} to the {name} checkout (looked at {})",
        path.display()
    );
}

fn parse_addr(s: &str, label: &str) -> Result<Address> {
    s.parse()
        .with_context(|| format!("parse {label} address {s}"))
}

#[cfg(test)]
fn find_addr(output: &str, label: &str) -> Result<Address> {
    let lines: Vec<&str> = output.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed == label
            && let Some(next) = lines.get(i + 1)
            && let Ok(addr) = next.trim().parse::<Address>()
        {
            return Ok(addr);
        }
        if let Some(rest) = trimmed.strip_prefix(label) {
            let rest = rest.trim();
            if let Ok(addr) = rest.parse::<Address>() {
                return Ok(addr);
            }
        }
    }
    bail!("did not find `{label} <address>` in forge output")
}

#[cfg(test)]
mod tests {
    use super::{find_addr, parse_deployed_to};
    use alloy::primitives::address;

    #[test]
    fn parses_console_log_same_line() {
        let out = "  Multicall3 0x00000000000000000000000000000000000000aa\n";
        assert_eq!(
            find_addr(out, "Multicall3").unwrap(),
            address!("0x00000000000000000000000000000000000000aa")
        );
    }

    #[test]
    fn parses_console_log_next_line() {
        let out = "SimpleAccount implementation\n0x00000000000000000000000000000000000000bb\n";
        assert_eq!(
            find_addr(out, "SimpleAccount implementation").unwrap(),
            address!("0x00000000000000000000000000000000000000bb")
        );
    }

    #[test]
    fn parses_forge_create_deployed_to() {
        let out = "Deployer: 0x00000000000000000000000000000000000000aa\nDeployed to: 0x00000000000000000000000000000000000000cc\n";
        assert_eq!(
            parse_deployed_to(out, "multicall").unwrap(),
            address!("0x00000000000000000000000000000000000000cc")
        );
    }
}
