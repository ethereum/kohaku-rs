use kohaku_railgun::chain_config::ChainConfig;
use wasm_bindgen::prelude::*;

use crate::bigint::JsU64;

/// A known Railgun deployment and its contract configuration.
#[wasm_bindgen]
pub struct Chain {
    pub(crate) inner: ChainConfig,
}

#[wasm_bindgen]
impl Chain {
    #[wasm_bindgen(js_name = mainnet)]
    #[must_use]
    pub fn mainnet() -> Self {
        Self {
            inner: ChainConfig::mainnet(),
        }
    }

    #[wasm_bindgen(js_name = sepolia)]
    #[must_use]
    pub fn sepolia() -> Self {
        Self {
            inner: ChainConfig::sepolia(),
        }
    }

    /// Find a configured deployment. Unknown chains return undefined.
    #[wasm_bindgen(js_name = fromChainId)]
    pub fn from_chain_id(chain_id: JsU64) -> Result<Option<Self>, JsError> {
        Ok(ChainConfig::from_chain_id(u64::try_from(chain_id)?).map(|inner| Self { inner }))
    }

    #[wasm_bindgen(getter, js_name = chainId)]
    #[must_use]
    pub fn chain_id(&self) -> u64 {
        self.inner.id
    }

    #[wasm_bindgen(getter, js_name = railgunSmartWallet)]
    #[must_use]
    pub fn railgun_smart_wallet(&self) -> String {
        self.inner.railgun_smart_wallet.to_string()
    }

    #[wasm_bindgen(getter, js_name = wrappedBaseToken)]
    #[must_use]
    pub fn wrapped_base_token(&self) -> String {
        self.inner.wrapped_base_token.to_string()
    }

    #[wasm_bindgen(getter, js_name = unshieldFeeBps)]
    #[must_use]
    pub fn unshield_fee_bps(&self) -> u16 {
        self.inner.unshield_fee_bps
    }

    #[wasm_bindgen(getter, js_name = deploymentBlock)]
    #[must_use]
    pub fn deployment_block(&self) -> u64 {
        self.inner.deployment_block
    }

    #[wasm_bindgen(getter, js_name = poiStartBlock)]
    #[must_use]
    pub fn poi_start_block(&self) -> u64 {
        self.inner.poi_start_block
    }

    #[wasm_bindgen(getter, js_name = subsquidEndpoint)]
    #[must_use]
    pub fn subsquid_endpoint(&self) -> String {
        self.inner.subsquid_endpoint.clone()
    }

    #[wasm_bindgen(getter, js_name = poiEndpoint)]
    #[must_use]
    pub fn poi_endpoint(&self) -> String {
        self.inner.poi_endpoint.clone()
    }
}
