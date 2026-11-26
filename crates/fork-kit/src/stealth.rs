use alloy::{
    primitives::Bytes,
    providers::{DynProvider, Provider, ext::AnvilApi},
};
use kohaku_stealth::Deployment;

const ERC_5564_ANNOUNCER_RUNTIME: &str =
    include_str!("../fixtures/stealth/erc5564_announcer.runtime.hex");
const ERC_6538_REGISTRY_RUNTIME: &str =
    include_str!("../fixtures/stealth/erc6538_registry.runtime.hex");

/// Deploys a fresh ERC-5564 announcer and ERC-6538 registry contracts at their canonical addresses on an anvil node.
///
/// Returns a [`Deployment`] pointing at the deployed contracts.
///
/// # Errors
/// Returns an error if any contract fails to deploy
pub async fn deploy_stealth(provider: DynProvider) -> Result<Deployment, anyhow::Error> {
    provider
        .anvil_set_code(
            Deployment::ETHEREUM.announcer,
            ERC_5564_ANNOUNCER_RUNTIME.trim().parse::<Bytes>()?,
        )
        .await?;
    provider
        .anvil_set_code(
            Deployment::ETHEREUM.registry,
            ERC_6538_REGISTRY_RUNTIME.trim().parse::<Bytes>()?,
        )
        .await?;

    let chain_id = provider.get_chain_id().await?;

    Ok(Deployment {
        chain_id,
        start_block: 0,
        ..Deployment::ETHEREUM
    })
}
