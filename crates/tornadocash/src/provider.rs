use alloy::{primitives::B256, providers::Provider};
use ruint::aliases::U256;

use crate::{
    abis::tornado::Tornado,
    pool::{Asset, Pool},
};

#[async_trait::async_trait]
pub trait TornadoProviderExt: Provider {
    async fn is_spent(
        &self,
        pool: &Pool,
        nullifier_hash: B256,
    ) -> Result<bool, alloy::contract::Error>;
    async fn is_known_root(&self, pool: &Pool, root: B256) -> Result<bool, alloy::contract::Error>;
    async fn quote_wei_in_fee_token(
        &self,
        pool: &Pool,
        wei_amount: U256,
    ) -> Result<U256, alloy::contract::Error>;
}

#[async_trait::async_trait]
impl<P: Provider> TornadoProviderExt for P {
    async fn is_spent(
        &self,
        pool: &Pool,
        nullifier_hash: B256,
    ) -> Result<bool, alloy::contract::Error> {
        Tornado::new(pool.address, self)
            .isSpent(nullifier_hash)
            .call()
            .await
    }

    async fn is_known_root(&self, pool: &Pool, root: B256) -> Result<bool, alloy::contract::Error> {
        Tornado::new(pool.address, self)
            .isKnownRoot(root)
            .call()
            .await
    }

    async fn quote_wei_in_fee_token(
        &self,
        pool: &Pool,
        wei_amount: U256,
    ) -> Result<U256, alloy::contract::Error> {
        match pool.asset {
            Asset::Native { .. } => Ok(wei_amount),
            Asset::Erc20 { address, .. } => {
                Tornado::new(pool.address, self)
                    .quoteWeiInToken(address, wei_amount)
                    .call()
                    .await
            }
        }
    }
}
