use std::collections::HashMap;

use js_sys::Map;
use kohaku_tornadocash_relayer::RelayerStatus as CoreRelayerStatus;
use serde::Serialize;
use tsify::{Ts, Tsify};
use wasm_bindgen::prelude::*;

use crate::{
    bigint::{JsU32, JsU64, JsU128, JsU256},
    hex::Hex,
    payer::Payer,
    pool::Pool,
};

/// A caller-supplied relayer status, used for offline pool support and fee quotes.
///
/// See [`kohaku_tornadocash_relayer::RelayerStatus`] for more details.
#[wasm_bindgen]
pub struct RelayerStatus {
    inner: CoreRelayerStatus,
}

/// A snapshot of the pool instances supported for one asset.
#[derive(Serialize, Tsify)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::struct_field_names,
    reason = "preserves the canonical Instance field name"
)]
pub struct Instance {
    instance_address: HashMap<String, Hex>,
    symbol: String,
    decimals: u8,
}

/// A snapshot of a relayer's supported instances, keyed by the core's asset key.
#[derive(Serialize, Tsify)]
pub struct Instances(HashMap<String, Instance>);

/// A snapshot of the relayer's reported health, preserving absent values.
#[derive(Serialize, Tsify)]
pub struct Health {
    status: Option<String>,
    error: Option<String>,
}

#[wasm_bindgen]
impl RelayerStatus {
    /// Parse the original JSON response body using the core's serde contract.
    ///
    /// Supply raw text to preserve integers before JavaScript number rounding.
    /// # Errors
    /// Throws if the body cannot be deserialized as a canonical relayer status.
    pub fn parse(json: &str) -> Result<Self, JsError> {
        Ok(Self {
            inner: serde_json::from_str(json)?,
        })
    }

    /// Return the reward account as checksummed hex.
    #[wasm_bindgen(getter, js_name = rewardAccount)]
    pub fn reward_account(&self) -> Result<Ts<Hex>, JsError> {
        Ok(Hex::from(self.inner.reward_account).into_ts()?)
    }

    /// Return a detached Map of assets to typed instance metadata.
    #[wasm_bindgen(getter)]
    pub fn instances(&self) -> Result<Ts<Instances>, JsError> {
        Ok(Instances(
            self.inner
                .instances
                .iter()
                .map(|(key, value)| {
                    (
                        key.clone(),
                        Instance {
                            instance_address: value
                                .instance_address
                                .iter()
                                .map(|(amount, address)| (amount.clone(), Hex::from(*address)))
                                .collect(),
                            symbol: value.symbol.clone(),
                            decimals: value.decimals,
                        },
                    )
                })
                .collect(),
        )
        .into_ts()?)
    }

    /// Return the network ID as an exact bigint.
    #[wasm_bindgen(getter, js_name = netId)]
    #[must_use]
    pub fn net_id(&self) -> JsU64 {
        self.inner.net_id.into()
    }

    /// Return a detached Map of exact token prices, keyed by asset symbol.
    #[wasm_bindgen(getter, js_name = ethPrices, unchecked_return_type = "Map<string, bigint>")]
    #[must_use]
    pub fn eth_prices(&self) -> Map {
        let prices = Map::new();
        for (symbol, price) in &self.inner.eth_prices {
            prices.set(&JsValue::from_str(symbol), &JsU256::from(*price));
        }
        prices
    }

    /// Return the reported Tornado service fee percentage.
    #[wasm_bindgen(getter, js_name = tornadoServiceFee)]
    #[must_use]
    pub fn tornado_service_fee(&self) -> f64 {
        self.inner.tornado_service_fee
    }

    /// Return the reported mining service fee percentage.
    #[wasm_bindgen(getter, js_name = miningServiceFee)]
    #[must_use]
    pub fn mining_service_fee(&self) -> f64 {
        self.inner.mining_service_fee
    }

    /// Return the reported relayer version.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn version(&self) -> String {
        self.inner.version.clone()
    }

    /// Return detached health metadata with undefined for absent values.
    #[wasm_bindgen(getter)]
    pub fn health(&self) -> Result<Ts<Health>, JsError> {
        Ok(Health {
            status: self.inner.health.status.clone(),
            error: self.inner.health.error.clone(),
        }
        .into_ts()?)
    }

    /// Return the reported queue size.
    #[wasm_bindgen(getter, js_name = currentQueue)]
    #[must_use]
    pub fn current_queue(&self) -> JsU32 {
        self.inner.current_queue.into()
    }

    /// Check pool support using the core, borrowing the pool without consuming it.
    #[must_use]
    pub fn supports(&self, pool: &Pool) -> bool {
        self.inner.supports(&pool.inner)
    }

    /// Return an independently owned canonical payer, borrowing the pool.
    ///
    /// # Errors
    /// Throws for invalid bigint widths or core quote errors.
    pub fn quote(
        &self,
        pool: &Pool,
        #[wasm_bindgen(js_name = gasPrice)] gas_price: JsU128,
        refund: JsU256,
    ) -> Result<Payer, JsError> {
        Ok(Payer {
            inner: self
                .inner
                .quote(&pool.inner, gas_price.try_into()?, refund.try_into()?)?,
        })
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use alloy::primitives::Address;
    use js_sys::Reflect;
    use kohaku_tornadocash_relayer::Instance as CoreInstance;
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::*;

    #[wasm_bindgen_test]
    fn instances_preserve_asset_amount_and_address() {
        let address = Address::with_last_byte(0xab);
        let status = RelayerStatus {
            inner: CoreRelayerStatus {
                instances: HashMap::from([(
                    "token".to_owned(),
                    CoreInstance {
                        instance_address: HashMap::from([("7".to_owned(), address)]),
                        symbol: "TKN".to_owned(),
                        ..Default::default()
                    },
                )]),
                ..Default::default()
            },
        };
        let instances: JsValue = status.instances().unwrap().into();
        let instance = Map::from(instances).get(&JsValue::from_str("token"));
        let addresses = Reflect::get(&instance, &JsValue::from_str("instanceAddress")).unwrap();
        assert_eq!(
            Map::from(addresses)
                .get(&JsValue::from_str("7"))
                .as_string()
                .unwrap(),
            address.to_string()
        );
    }
}
