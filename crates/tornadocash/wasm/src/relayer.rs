use std::rc::Rc;

use js_sys::Promise;
use kohaku_tornadocash_relayer::Relayer as CoreRelayer;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use crate::{
    Payer, Pool, RelayerStatus,
    bigint::{JsU128, JsU256},
};

/// HTTP client for a Tornado Cash relayer.
#[wasm_bindgen]
pub struct Relayer {
    pub(crate) inner: Rc<CoreRelayer>,
}

#[wasm_bindgen]
impl Relayer {
    #[wasm_bindgen(constructor)]
    #[must_use]
    pub fn new(url: &str) -> Self {
        Self {
            inner: Rc::new(CoreRelayer::new(url)),
        }
    }

    /// Fetch the relayer's current status.
    #[wasm_bindgen(unchecked_return_type = "Promise<RelayerStatus>")]
    pub fn status(&self) -> Promise {
        let client = self.inner.clone();
        future_to_promise(async move {
            let inner = client.status().await.map_err(JsError::from)?;
            Ok(RelayerStatus { inner }.into())
        })
    }

    /// Fetch a quote using the relayer's current status.
    #[wasm_bindgen(unchecked_return_type = "Promise<Payer>")]
    pub fn quote(
        &self,
        pool: &Pool,
        #[wasm_bindgen(js_name = gasPrice)] gas_price: JsU128,
        refund: JsU256,
    ) -> Promise {
        let client = self.inner.clone();
        let pool = pool.inner.clone();
        future_to_promise(async move {
            let payer = client
                .quote(&pool, gas_price.try_into()?, refund.try_into()?)
                .await
                .map_err(JsError::from)?;
            Ok(Payer::from(payer).into())
        })
    }
}
