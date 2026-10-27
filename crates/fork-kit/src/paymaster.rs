//! 4337 [PrivacyPaymaster](https://github.com/Robert-MacWha/privacy-paymaster) deployment and configuration.

use alloy::{
    primitives::{Address, U256},
    providers::DynProvider,
};

use crate::paymaster::sol::PrivacyPaymaster;

mod sol {
    use alloy::sol;

    sol!(
        #[sol(rpc)]
        PrivacyPaymaster,
        "fixtures/privacy_paymaster.json"
    );
}

const PAYMASTER_STAKE_WEI: u128 = 10_u128.pow(17); // 0.1 ETH
const PAYMASTER_DEPOSIT_WEI: u128 = 10_u128.pow(17); // 0.1 ETH
const PAYMASTER_UNSTAKE_DELAY_SEC: u32 = 3600;
const PAYMASTER_TWAP_PERIOD_SEC: u32 = 3600;

/// Deploys and stakes a new `PrivacyPaymaster`.
///
/// Returns the address of the deployed paymaster.
///
/// # Errors
/// Returns an error if the deploy, deposit, or stake transactions fail.
pub async fn deploy_paymaster(
    provider: DynProvider,
    entrypoint: Address,
    factory: Address,
    weth: Address,
) -> Result<Address, anyhow::Error> {
    let paymaster = sol::PrivacyPaymaster::deploy(
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
        .watch()
        .await?;
    paymaster
        .addStake(PAYMASTER_UNSTAKE_DELAY_SEC)
        .value(U256::from(PAYMASTER_STAKE_WEI))
        .send()
        .await?
        .watch()
        .await?;

    Ok(*paymaster.address())
}

/// Allows a tornadocash pool instance on the given paymaster
pub async fn allow_instance(
    provider: DynProvider,
    paymaster: Address,
    pool_address: Address,
) -> Result<(), anyhow::Error> {
    let paymaster = PrivacyPaymaster::new(paymaster, provider);
    paymaster
        .setInstance(pool_address, true)
        .send()
        .await?
        .watch()
        .await?;

    Ok(())
}
