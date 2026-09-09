#![allow(dead_code)]

use alloy::{
    primitives::U256,
    providers::{DynProvider, Provider},
    sol,
};
use kohaku_tornadocash::provider::pool::{Asset, Pool};

sol!(
    #[sol(rpc)]
    Hasher,
    "tests/fixtures/hasher.json"
);

sol!(
    #[sol(rpc)]
    Verifier,
    "tests/fixtures/verifier.json"
);

sol!(
    #[sol(rpc)]
    ETHTornado,
    "tests/fixtures/eth_tornado.json"
);

const DENOMINATION_WEI: u128 = 10_u128.pow(17);
const MERKLE_TREE_HEIGHT: u32 = 20;

/// Deploy a fresh Hasher, Verifier, and ETHTornado instance to `provider` and return the
/// corresponding [`Pool`].
pub async fn deploy_local_pool(provider: DynProvider) -> Result<Pool, anyhow::Error> {
    let hasher = Hasher::deploy(&provider).await?;
    let verifier = Verifier::deploy(&provider).await?;
    let tornado = ETHTornado::deploy(
        &provider,
        *verifier.address(),
        *hasher.address(),
        U256::from(DENOMINATION_WEI),
        MERKLE_TREE_HEIGHT,
    )
    .await?;

    Ok(Pool {
        chain_id: provider.get_chain_id().await?,
        address: *tornado.address(),
        asset: Asset::Native {
            symbol: "ETH",
            decimals: 18,
        },
        amount_wei: DENOMINATION_WEI,
        deployed_block: 0,
        paymaster_address: None,
        adapter_address: None,
    })
}
