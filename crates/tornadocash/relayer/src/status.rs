use std::collections::HashMap;

use alloy::primitives::{Address, U256};
use kohaku_tornadocash::{Asset, Payer, Pool};
use ruint::uint;
use serde::{Deserialize, Serialize};

use crate::RelayerError;

/// A relayer's status.
///
/// Response from `v1/status`. See [tornado-relayer/src/contollers/status.js](https://github.com/tornado-dao/tornado-relayer/blob/52473197ea49fb70dab8fead01de52545801ca6b/src/contollers/status.js#L13)
/// for the reference.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayerStatus {
    pub reward_account: Address,
    pub instances: HashMap<String, Instance>,
    pub net_id: u64,
    pub eth_prices: HashMap<String, U256>,
    pub tornado_service_fee: f64,
    pub mining_service_fee: f64,
    pub version: String,
    pub health: Health,
    pub current_queue: u32,
}

/// A pool instance supported by a relayer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub instance_address: HashMap<String, Address>,
    pub symbol: String,
    pub decimals: u8,
}

/// The health of a relayer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Health {
    pub status: Option<String>,
    pub error: Option<String>,
}

impl RelayerStatus {
    /// Checks if the relayer supports the given pool.
    #[must_use]
    pub fn supports(&self, pool: &Pool) -> bool {
        if pool.chain_id != self.net_id {
            return false;
        }

        let Some(instance) = self.instances.get(pool.symbol()) else {
            return false;
        };

        instance.instance_address.get(&pool.amount()) == Some(&pool.address)
    }

    /// Quotes the fee for a withdrawal paid by the relayer.
    ///
    /// # Errors
    /// Returns an error if the relayer does not support the given pool, or an ERC20
    /// quote has a zero price or overflows an intermediate or final value.
    pub fn quote(&self, pool: &Pool, gas_price: u128, refund: U256) -> Result<Payer, RelayerError> {
        // Scale the fee percentage into a fixed-point integer.
        const FEE_PRECISION: u64 = 1_000_000;
        const WITHDRAWAL_GAS: U256 = uint!(500_000_U256);

        if !self.supports(pool) {
            return Err(RelayerError::UnsupportedPool(pool.clone()));
        }

        #[expect(clippy::cast_sign_loss, clippy::cast_precision_loss)]
        let fee_scaled = (self.tornado_service_fee / 100.0 * FEE_PRECISION as f64).round() as u64;
        let fee_percent =
            (U256::from(pool.amount_wei) * U256::from(fee_scaled)) / U256::from(FEE_PRECISION);
        let expense = U256::from(gas_price) * WITHDRAWAL_GAS;

        // If the asset is native, the fee is `expense + fee_percent`
        if matches!(pool.asset, Asset::Native { .. }) {
            return Ok(Payer {
                address: self.reward_account,
                fee: expense + fee_percent,
                refund,
            });
        }

        let Some(price) = self.eth_prices.get(pool.symbol()).copied() else {
            return Err(RelayerError::UnsupportedPool(pool.clone()));
        };

        if price.is_zero() {
            return Err(RelayerError::UnsupportedPool(pool.clone()));
        }

        // Preserve the ERC20 formula and its operation order, checking each step.
        let expense_with_refund = expense
            .checked_add(refund)
            .ok_or(RelayerError::QuoteOverflow)?;
        let scale = U256::from(10)
            .checked_pow(U256::from(pool.asset.decimals()))
            .ok_or(RelayerError::QuoteOverflow)?;
        let fee = expense_with_refund
            .checked_mul(scale)
            .and_then(|value| value.checked_div(price))
            .and_then(|value| value.checked_add(fee_percent))
            .ok_or(RelayerError::QuoteOverflow)?;
        Ok(Payer {
            address: self.reward_account,
            fee,
            refund,
        })
    }
}

#[cfg(test)]
mod tests {
    use alloy::primitives::address;

    use super::*;

    #[test]
    fn test_supports() {
        let pool = Pool::ETHEREUM_DAI_100;

        let status = RelayerStatus {
            instances: HashMap::from([(
                pool.symbol().to_string(),
                Instance {
                    instance_address: HashMap::from([(pool.amount(), pool.address)]),
                    symbol: pool.symbol().to_string(),
                    decimals: pool.asset.decimals(),
                },
            )]),
            net_id: pool.chain_id,
            ..Default::default()
        };

        assert!(status.supports(&pool));

        let mut different_net_id_pool = pool.clone();
        different_net_id_pool.chain_id = 2;
        assert!(!status.supports(&different_net_id_pool));

        let mut different_address_pool = pool.clone();
        different_address_pool.address = address!("0x000000000000000000000000000000000000cafe");
        assert!(!status.supports(&different_address_pool));

        let mut different_amount_pool = pool.clone();
        different_amount_pool.amount_wei = 2_000_000u64.into();
        assert!(!status.supports(&different_amount_pool));
    }

    #[test]
    fn test_fee_eth() {
        let pool = Pool::ETHEREUM_ETHER_1;

        let status = RelayerStatus {
            tornado_service_fee: 1.0,
            eth_prices: HashMap::from([(pool.symbol().to_string(), U256::from(pool.amount_wei))]),
            instances: HashMap::from([(
                pool.symbol().to_string(),
                Instance {
                    instance_address: HashMap::from([(pool.amount(), pool.address)]),
                    symbol: pool.symbol().to_string(),
                    decimals: pool.asset.decimals(),
                },
            )]),
            net_id: pool.chain_id,
            ..Default::default()
        };

        let fee = status
            .quote(&pool, 1_000_000_000u128, U256::ZERO)
            .unwrap()
            .fee;

        // 1% of 1 ETH + (1 gwei * 500,000 gas)
        assert_eq!(fee, U256::from(10_500_000_000_000_000u64));
    }

    #[test]
    fn test_fee_erc20() {
        let pool = Pool::ETHEREUM_DAI_100;

        let status = RelayerStatus {
            tornado_service_fee: 1.0,
            eth_prices: HashMap::from([(
                pool.symbol().to_string(),
                U256::from(1_000_000_000_000_000_000u64),
            )]),
            instances: HashMap::from([(
                pool.symbol().to_string(),
                Instance {
                    instance_address: HashMap::from([(pool.amount(), pool.address)]),
                    symbol: pool.symbol().to_string(),
                    decimals: pool.asset.decimals(),
                },
            )]),
            net_id: pool.chain_id,
            ..Default::default()
        };

        let fee = status
            .quote(&pool, 1_000_000_000u128, U256::from(100u64))
            .unwrap()
            .fee;

        // 1% of 100 DAI + ((1 gwei * 500,000 gas) + 100 refund) valued in DAI
        assert_eq!(fee, U256::from(1_000_500_000_000_000_100u64));
    }

    fn dai_status(price: U256) -> RelayerStatus {
        let pool = Pool::ETHEREUM_DAI_100;
        RelayerStatus {
            net_id: pool.chain_id,
            tornado_service_fee: 1.0,
            eth_prices: HashMap::from([(pool.symbol().to_owned(), price)]),
            instances: HashMap::from([(
                pool.symbol().to_owned(),
                Instance {
                    instance_address: HashMap::from([(pool.amount(), pool.address)]),
                    ..Default::default()
                },
            )]),
            ..Default::default()
        }
    }

    #[test]
    fn erc20_quote_rejects_expense_plus_refund_overflow() {
        let status = dai_status(U256::ONE);
        assert!(matches!(
            status.quote(&Pool::ETHEREUM_DAI_100, 1, U256::MAX),
            Err(RelayerError::QuoteOverflow)
        ));
    }

    #[test]
    fn erc20_quote_rejects_multiplication_overflow_before_division() {
        let status = dai_status(U256::MAX);
        // Even if division would reduce the result, preserve the formula's order.
        assert!(matches!(
            status.quote(&Pool::ETHEREUM_DAI_100, 0, U256::MAX),
            Err(RelayerError::QuoteOverflow)
        ));
    }

    #[test]
    fn erc20_quote_rejects_final_fee_overflow() {
        let status = dai_status(U256::ONE);
        // This refund scales without overflowing, but adding the service fee overflows.
        let refund = U256::MAX / U256::from(1_000_000_000_000_000_000_u64);
        assert!(matches!(
            status.quote(&Pool::ETHEREUM_DAI_100, 0, refund),
            Err(RelayerError::QuoteOverflow)
        ));
    }
}
