use kohaku_tornadocash::event::Snapshot as CoreSnapshot;
use tsify::{Ts, Tsify};
use wasm_bindgen::prelude::*;

use crate::{events::DecodedEvent, pool::Pool};

/// A pool's events and the actual half-open block range covered by a syncer.
///
/// See [`kohaku_tornadocash::event::Snapshot`] for more details.
#[wasm_bindgen]
pub struct Snapshot {
    pub(crate) inner: CoreSnapshot,
}

#[wasm_bindgen]
impl Snapshot {
    /// Return an independently owned pool wrapper.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn pool(&self) -> Pool {
        self.inner.pool().clone().into()
    }

    /// Return the inclusive start of the actual covered range as an exact bigint.
    #[wasm_bindgen(getter, js_name = fromBlock)]
    #[must_use]
    pub fn from_block(&self) -> u64 {
        self.inner.range().start
    }

    /// Return the exclusive end of the actual covered range as an exact bigint.
    #[wasm_bindgen(getter, js_name = toBlock)]
    #[must_use]
    pub fn to_block(&self) -> u64 {
        self.inner.range().end
    }

    /// Return detached plain event objects in the core's order.
    ///
    /// # Errors
    /// Throws if an event cannot be represented as a JavaScript value.
    #[wasm_bindgen(getter)]
    pub fn events(&self) -> Result<Vec<Ts<DecodedEvent>>, JsError> {
        self.inner
            .events()
            .iter()
            .cloned()
            .map(|event| Ok(DecodedEvent::from(event).into_ts()?))
            .collect()
    }
}

impl From<CoreSnapshot> for Snapshot {
    fn from(inner: CoreSnapshot) -> Self {
        Self { inner }
    }
}
