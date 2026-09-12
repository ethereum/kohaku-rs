use alloy::{
    primitives::{Address, U256},
    providers::DynProvider,
    sol,
};

sol!(
    #[sol(rpc)]
    PrivacyPaymaster,
    "fixtures/privacy_paymaster.json"
);
sol!(
    #[sol(rpc)]
    TornadoFeeAdapter,
    "fixtures/tornado_fee_adapter.json"
);

const PAYMASTER_STAKE_WEI: u128 = 10_u128.pow(17); // 0.1 ETH
const PAYMASTER_DEPOSIT_WEI: u128 = 10_u128.pow(17); // 0.1 ETH
const PAYMASTER_UNSTAKE_DELAY_SEC: u32 = 3600;
const PAYMASTER_TWAP_PERIOD_SEC: u32 = 3600;

/// Deploys a `PrivacyPaymaster` bound to `entrypoint`, `factory`, and `weth`, then deposits and
/// stakes it so it is immediately usable. Returns the paymaster's address.
///
/// # Errors
/// Returns an error if the deploy, deposit, or stake transactions fail.
pub async fn deploy_paymaster(
    provider: DynProvider,
    entrypoint: Address,
    factory: Address,
    weth: Address,
) -> Result<Address, anyhow::Error> {
    let paymaster = PrivacyPaymaster::deploy(
        &provider,
        entrypoint,
        factory,
        weth,
        PAYMASTER_TWAP_PERIOD_SEC,
    )
    .await?;

    paymaster
        .deposit()
        .value(U256::from(PAYMASTER_DEPOSIT_WEI))
        .send()
        .await?
        .get_receipt()
        .await?;
    paymaster
        .addStake(PAYMASTER_UNSTAKE_DELAY_SEC)
        .value(U256::from(PAYMASTER_STAKE_WEI))
        .send()
        .await?
        .get_receipt()
        .await?;

    Ok(*paymaster.address())
}

/// Deploys a `TornadoFeeAdapter` for `pool_address` and approves it on `paymaster`. Returns the
/// adapter's address.
///
/// # Errors
/// Returns an error if the deploy or approval transactions fail.
pub async fn deploy_fee_adapter(
    provider: DynProvider,
    paymaster: Address,
    pool_address: Address,
) -> Result<Address, anyhow::Error> {
    let adapter = TornadoFeeAdapter::deploy(&provider, pool_address).await?;
    PrivacyPaymaster::new(paymaster, &provider)
        .setApprovedAdapter(*adapter.address(), true)
        .send()
        .await?
        .get_receipt()
        .await?;

    Ok(*adapter.address())
}
