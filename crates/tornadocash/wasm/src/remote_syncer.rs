use js_sys::Promise;
use kohaku_tornadocash_syncer::{DynSyncer, Syncer, remote::RemoteSyncer as CoreRemoteSyncer};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use crate::{bigint::JsU64, pool::Pool, snapshot::Snapshot};

/// A syncer that retrieves a pool's events from a compatible remote cache.
///
/// See [`kohaku_tornadocash_syncer::remote::RemoteSyncer`] for cache naming and format.
#[wasm_bindgen]
pub struct RemoteSyncer {
    pub(crate) inner: DynSyncer,
}

#[wasm_bindgen]
impl RemoteSyncer {
    /// Construct a client for the caller-supplied cache directory URL.
    #[wasm_bindgen(constructor)]
    #[must_use]
    pub fn new(#[wasm_bindgen(js_name = baseUrl)] base_url: &str) -> Self {
        Self {
            inner: DynSyncer::new(CoreRemoteSyncer::new(base_url)),
        }
    }

    /// Retrieve events for a half-open range, preserving the core's actual covered range.
    ///
    /// The request owns its client and pool handles before returning the `Promise`;
    /// the original wrappers may be freed while it is pending.
    ///
    /// # Errors
    /// Rejects if either endpoint is not a u64 bigint or the core request fails.
    #[wasm_bindgen(js_name = syncRange, unchecked_return_type = "Promise<Snapshot>")]
    pub fn sync_range(
        &self,
        pool: &Pool,
        #[wasm_bindgen(js_name = fromBlock)] from_block: JsU64,
        #[wasm_bindgen(js_name = toBlock)] to_block: JsU64,
    ) -> Promise {
        let client = self.inner.clone();
        let pool = pool.inner.clone();
        future_to_promise(async move {
            let from_block = u64::try_from(from_block)?;
            let to_block = u64::try_from(to_block)?;
            let snapshot = client
                .sync_range(&pool, from_block, to_block)
                .await
                .map_err(JsError::from)?;
            Ok(Snapshot::from(snapshot).into())
        })
    }
}
