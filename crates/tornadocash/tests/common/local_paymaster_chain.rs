#![allow(dead_code)]

use alloy::{
    network::TransactionBuilder,
    primitives::{Address, Bytes, U256, address},
    providers::{DynProvider, Provider},
    rpc::types::TransactionRequest,
    sol,
};
use kohaku_tornadocash::provider::pool::Pool;
use kohaku_userop_kit::entry_point::ENTRY_POINT_08;

use crate::common::local_chain::deploy_local_pool;

sol!(
    #[sol(rpc)]
    PrivacyPaymaster,
    "tests/fixtures/privacy_paymaster.json"
);

sol!(
    #[sol(rpc)]
    TornadoFeeAdapter,
    "tests/fixtures/tornado_fee_adapter.json"
);

const DETERMINISTIC_DEPLOYER: Address = address!("0x4e59b44847b379578588920ca78fbf26c0b4956c");

// CREATECALL blobs (salt + init code) pulled from [`test/e2e/deploy-contracts/constants.ts`](https://github.com/pimlicolabs/alto/blob/main/test/e2e/deploy-contracts/constants.ts).
const ENTRY_POINT_V08_CREATECALL: &str = include_str!("../fixtures/entry_point_v08.createcall.hex");
const SIMPLE_7702_ACCOUNT_IMPLEMENTATION_V08_CREATECALL: &str =
    include_str!("../fixtures/simple_7702_account_v08.createcall.hex");

/// Expected deterministic canonical address of the `Simple7702Account`.
const SIMPLE_7702_ACCOUNT_IMPLEMENTATION_V08: Address =
    address!("0xe6Cae83BdE06E4c305530e199D7217f42808555B");

const PAYMASTER_STAKE_WEI: u128 = 10_u128.pow(17); // 0.1 ETH
const PAYMASTER_DEPOSIT_WEI: u128 = 10_u128.pow(17); // 0.1 ETH
const PAYMASTER_UNSTAKE_DELAY_SEC: u32 = 3600;
const PAYMASTER_TWAP_PERIOD_SEC: u32 = 3600;

/// Deploys a fresh ETHTornado pool ([`deploy_local_pool`]) and a
/// `PrivacyPaymaster`/`TornadoFeeAdapter` for it, returning the `Pool` with
/// `paymaster_address`/`adapter_address` populated.
pub async fn deploy_local_pool_with_paymaster(
    provider: DynProvider,
) -> Result<Pool, anyhow::Error> {
    deploy_erc4337_v08_infra(&provider).await?;
    let mut pool = deploy_local_pool(provider.clone()).await?;

    let placeholder_factory = address!("0x0000000000000000000000000000000000000011");
    let placeholder_weth = address!("0x0000000000000000000000000000000000000022");

    let paymaster = PrivacyPaymaster::deploy(
        &provider,
        ENTRY_POINT_08,
        placeholder_factory,
        placeholder_weth,
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

    let adapter = TornadoFeeAdapter::deploy(&provider, pool.address).await?;
    paymaster
        .setApprovedAdapter(*adapter.address(), true)
        .send()
        .await?
        .get_receipt()
        .await?;

    pool.paymaster_address = Some(*paymaster.address());
    pool.adapter_address = Some(*adapter.address());
    Ok(pool)
}

/// Deploys the v0.8 `EntryPoint` and `Simple7702Account` implementation to the provided `provider`
/// at their canonical addresses.
async fn deploy_erc4337_v08_infra(provider: &DynProvider) -> Result<(), anyhow::Error> {
    for blob in [
        ENTRY_POINT_V08_CREATECALL,
        SIMPLE_7702_ACCOUNT_IMPLEMENTATION_V08_CREATECALL,
    ] {
        let data: Bytes = blob.trim().parse()?;
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
    }

    for address in [ENTRY_POINT_08, SIMPLE_7702_ACCOUNT_IMPLEMENTATION_V08] {
        anyhow::ensure!(
            !provider.get_code_at(address).await?.is_empty(),
            "expected contract code at {address} after deploying v0.8 ERC-4337 infra"
        );
    }

    Ok(())
}
