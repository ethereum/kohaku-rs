use alloy::{
    network::TransactionBuilder,
    primitives::{Address, Bytes, address},
    providers::{DynProvider, Provider},
    rpc::types::TransactionRequest,
};

const DETERMINISTIC_DEPLOYER: Address = address!("0x4e59b44847b379578588920ca78fbf26c0b4956c");

// CREATECALL blob (salt + init code) pulled from
// test/e2e/deploy-contracts/constants.ts in pimlicolabs/alto.
const SIMPLE_7702_ACCOUNT_IMPLEMENTATION_V08_CREATECALL: &str =
    include_str!("../fixtures/simple_7702_account_v08.createcall.hex");

/// Expected deterministic canonical address of the `Simple7702Account` implementation.
pub const SIMPLE_7702_ACCOUNT_IMPLEMENTATION_V08: Address =
    address!("0xe6Cae83BdE06E4c305530e199D7217f42808555B");

/// Deploys the ERC-7702 `Simple7702Account` implementation to `provider` at
/// [`SIMPLE_7702_ACCOUNT_IMPLEMENTATION_V08`] and returns that address.
///
/// # Errors
/// Returns an error if the deploy transaction fails, or if no code is found at
/// [`SIMPLE_7702_ACCOUNT_IMPLEMENTATION_V08`] afterward.
pub async fn deploy_simple_account(provider: &DynProvider) -> Result<Address, anyhow::Error> {
    let data: Bytes = SIMPLE_7702_ACCOUNT_IMPLEMENTATION_V08_CREATECALL
        .trim()
        .parse()?;
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
        !provider
            .get_code_at(SIMPLE_7702_ACCOUNT_IMPLEMENTATION_V08)
            .await?
            .is_empty(),
        "expected contract code at {SIMPLE_7702_ACCOUNT_IMPLEMENTATION_V08} after deploying \
         Simple7702Account implementation"
    );

    Ok(SIMPLE_7702_ACCOUNT_IMPLEMENTATION_V08)
}
