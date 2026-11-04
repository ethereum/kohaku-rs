use alloy::{providers::RootProvider, transports::http::reqwest::Url};
use js_sys::{Array, Promise};
use kohaku_tornadocash::{Field, TornadoProviderExt};
use tsify::Ts;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use crate::{Pool, hex::Hex};

/// Read-only Tornado pool queries through an HTTP(S) RPC endpoint.
///
/// See [`kohaku_tornadocash::TornadoProviderExt`] for canonical query semantics.
/// Calls snapshot their arguments and retain a provider handle before returning
/// a Promise, so wrappers may be freed while a request is pending.
#[wasm_bindgen]
pub struct TornadoProvider {
    // Alloy RPC client; Tornado queries are provided by TornadoProviderExt.
    inner: RootProvider,
}

#[wasm_bindgen]
impl TornadoProvider {
    /// Construct an Alloy HTTP provider without making a network request.
    ///
    /// # Errors
    ///
    /// Throws if `url` is invalid or does not use HTTP(S).
    #[wasm_bindgen(constructor)]
    pub fn new(url: &str) -> Result<Self, JsError> {
        let url = Url::parse(url)?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(JsError::new("RPC URL must use http or https"));
        }
        Ok(Self {
            inner: RootProvider::new_http(url),
        })
    }

    /// Query whether a nullifier hash has been spent.
    ///
    /// Rejects on invalid field hex or an RPC failure.
    #[wasm_bindgen(js_name = isSpent, unchecked_return_type = "Promise<boolean>")]
    pub fn is_spent(&self, pool: &Pool, hash: &Ts<Hex>) -> Promise {
        let hash = decode_field(hash);
        let pool = pool.inner.clone();
        let provider = self.inner.clone();
        future_to_promise(async move {
            let result = provider
                .is_spent(&pool, hash?)
                .await
                .map_err(JsError::from)?;
            Ok(JsValue::from(result))
        })
    }

    /// Query spent status in the same length and order as the supplied hashes.
    ///
    /// Rejects on invalid field hex or an RPC failure.
    #[wasm_bindgen(js_name = isSpentArray, unchecked_return_type = "Promise<boolean[]>")]
    pub fn is_spent_array(&self, pool: &Pool, hashes: Vec<Ts<Hex>>) -> Promise {
        let hashes: Result<Vec<_>, _> = hashes
            .into_iter()
            .map(|value| decode_field(&value))
            .collect();
        let pool = pool.inner.clone();
        let provider = self.inner.clone();
        future_to_promise(async move {
            let result = provider
                .is_spent_array(&pool, &hashes?)
                .await
                .map_err(JsError::from)?;
            Ok(result
                .into_iter()
                .map(JsValue::from)
                .collect::<Array>()
                .into())
        })
    }

    /// Query whether a Merkle root is known to the pool.
    ///
    /// Rejects on invalid field hex or an RPC failure.
    #[wasm_bindgen(js_name = isKnownRoot, unchecked_return_type = "Promise<boolean>")]
    pub fn is_known_root(&self, pool: &Pool, root: &Ts<Hex>) -> Promise {
        let root = decode_field(root);
        let pool = pool.inner.clone();
        let provider = self.inner.clone();
        future_to_promise(async move {
            let result = provider
                .is_known_root(&pool, root?)
                .await
                .map_err(JsError::from)?;
            Ok(JsValue::from(result))
        })
    }
}

fn decode_field(value: &Ts<Hex>) -> Result<Field, JsError> {
    Ok(value.to_rust()?.try_into()?)
}

#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_tests {
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::*;

    #[wasm_bindgen_test]
    fn restricts_transport_to_http_schemes() {
        for url in ["http://127.0.0.1:1", "https://example.invalid/rpc"] {
            assert!(TornadoProvider::new(url).is_ok());
        }
        assert!(TornadoProvider::new("ws://localhost:8545").is_err());
    }
}
