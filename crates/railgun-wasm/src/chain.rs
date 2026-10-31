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
}
