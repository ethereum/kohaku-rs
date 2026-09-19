//! Deploy MSP HEAD + FrameAccount factory on an 8-field Hegota node.

use alloy::{
    primitives::{Address, Bytes, B256},
    providers::Provider,
};
use anyhow::{Context, Result, bail};
use kohaku_frametx_kit::{HEGOTA_CHAIN_ID, FrameTxClient};

/// Addresses produced by [`deploy_minimal_shield_pool`].
#[derive(Debug, Clone)]
pub struct MinimalShieldDeployment {
    pub poseidon_t3: Address,
    pub poseidon_t4: Address,
    pub verifier: Address,
    pub logic: Address,
    pub pool: Address,
    pub factory: Address,
}

/// Refuse unless chain 8141, `slotNumber` is present, and testbed setup is explicit.
///
/// Contract bytecode is read from `fixtures/minimal-shield/*.hex` once those
/// artifacts are compiled from the MSP repo (`forge build` + `dispatcher.py --artifact`).
///
/// # Errors
/// Returns if the RPC is not Hegota or fixtures are missing.
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
    let _ = Bytes::new();
    let _ = B256::ZERO;
    bail!(
        "compile MSP contracts into crates/fork-kit/fixtures/minimal-shield/ and wire create() calls (PoseidonT3/T4, Groth16Verifier, ShieldedPoolLogic, dispatcher initcode, FrameAccountFactory)"
    )
}
