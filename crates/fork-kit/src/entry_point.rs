use alloy::{
    network::TransactionBuilder,
    primitives::{Address, Bytes, address},
    providers::{DynProvider, Provider},
    rpc::types::TransactionRequest,
};
use kohaku_userop_kit::entry_point::ENTRY_POINT_08;

const DETERMINISTIC_DEPLOYER: Address = address!("0x4e59b44847b379578588920ca78fbf26c0b4956c");

// CREATECALL blob (salt + init code) pulled from
// test/e2e/deploy-contracts/constants.ts in pimlicolabs/alto.
const ENTRY_POINT_V08_CREATECALL: &str = include_str!("../fixtures/entry_point_v08.createcall.hex");

/// Deploys the canonical ERC-4337 v0.8 `EntryPoint` to `provider` at [`ENTRY_POINT_08`] and
/// returns that address.
///
/// # Errors
/// Returns an error if the deploy transaction fails, or if no code is found at
/// [`ENTRY_POINT_08`] afterward.
pub async fn deploy_entry_point(provider: &DynProvider) -> Result<Address, anyhow::Error> {
    let data: Bytes = ENTRY_POINT_V08_CREATECALL.trim().parse()?;
    provider
        .send_transaction(
            TransactionRequest::default()
                .with_to(DETERMINISTIC_DEPLOYER)
                .with_input(data)
                .with_gas_limit(15_000_000),
        )
        .await?
        .get_receipt()
        .await?;

    anyhow::ensure!(
        !provider.get_code_at(ENTRY_POINT_08).await?.is_empty(),
        "expected contract code at {ENTRY_POINT_08} after deploying v0.8 EntryPoint"
    );

    Ok(ENTRY_POINT_08)
}
