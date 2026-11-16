//! Tornadocash pool deployment.

use alloy::{
    primitives::{Address, U256},
    providers::DynProvider,
};

mod sol {
    #![allow(clippy::all)]
    #![allow(clippy::pedantic)]

    use alloy::sol;

    sol!(
        #[sol(rpc)]
        Hasher,
        "fixtures/hasher.json"
    );
    sol!(
        #[sol(rpc)]
        Verifier,
        "fixtures/verifier.json"
    );
    sol!(
        #[sol(rpc)]
        ETHTornado,
        "fixtures/eth_tornado.json"
    );
    sol!(
        #[sol(rpc)]
        TornadoProxyLight,
        "fixtures/tornado_proxy_light.json"
    );
}

const MERKLE_TREE_HEIGHT: u32 = 20;

/// Deploys a fresh ETH tornadocash pool contract with a denomination of `denomination_wei`.
///
/// Returns the address of the deployed pool.
///
/// # Errors
/// Returns an error if any contract fails to deploy.
pub async fn deploy_pool(
    provider: DynProvider,
    denomination_wei: u128,
) -> Result<Address, anyhow::Error> {
    let hasher = sol::Hasher::deploy(&provider).await?;
    let verifier = sol::Verifier::deploy(&provider).await?;
    let tornado = sol::ETHTornado::deploy(
        &provider,
        *verifier.address(),
        *hasher.address(),
        U256::from(denomination_wei),
        MERKLE_TREE_HEIGHT,
    )
    .await?;

    Ok(*tornado.address())
}

/// Deploy a fresh `TornadoProxyLight` to `provider` and return its address.
///
/// # Errors
/// Returns an error if the contract fails to deploy.
pub async fn deploy_proxy(provider: DynProvider) -> Result<Address, anyhow::Error> {
    let proxy = sol::TornadoProxyLight::deploy(&provider).await?;
    Ok(*proxy.address())
}
