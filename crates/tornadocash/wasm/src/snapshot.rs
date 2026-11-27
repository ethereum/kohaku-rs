use kohaku_tornadocash::event::{Snapshot as CoreSnapshot, SyncEvent};
use tsify::{Ts, Tsify};
use wasm_bindgen::prelude::*;

use crate::{bigint::JsU64, events::DecodedEvent, pool::Pool};

/// A pool's events within a half-open block range.
///
/// See [`kohaku_tornadocash::event::Snapshot`] for more details.
#[wasm_bindgen]
pub struct Snapshot {
    pub(crate) inner: CoreSnapshot,
}

#[wasm_bindgen]
impl Snapshot {
    /// Construct a snapshot from events in their supplied order, borrowing the pool.
    ///
    /// # Errors
    /// Throws if block bounds are not u64 bigints, event fields cannot be decoded,
    /// or the core rejects out-of-range events or non-contiguous deposit indices.
    #[wasm_bindgen(constructor)]
    pub fn new(
        pool: &Pool,
        #[wasm_bindgen(js_name = fromBlock)] from_block: JsU64,
        #[wasm_bindgen(js_name = toBlock)] to_block: JsU64,
        events: Vec<Ts<DecodedEvent>>,
    ) -> Result<Self, JsError> {
        let range = u64::try_from(from_block)?..u64::try_from(to_block)?;
        let events = events
            .into_iter()
            .map(|event| SyncEvent::try_from(DecodedEvent::try_from(event)?))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(CoreSnapshot::new(pool.inner.clone(), range, events)?.into())
    }

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

#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_tests {
    use alloy::primitives::{Address, U256};
    use kohaku_tornadocash::{
        Field,
        event::{DepositEvent, SyncEvent, WithdrawalEvent},
    };
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::*;

    #[wasm_bindgen_test]
    fn reconstructs_snapshot_from_its_js_events() {
        let pool = Pool::known().pop().unwrap();
        let block = 9_007_199_254_740_993;
        let events = vec![
            SyncEvent::Deposit(DepositEvent {
                commitment: Field::from(1),
                leaf_index: 0,
                block_number: block,
            }),
            SyncEvent::Withdrawal(WithdrawalEvent {
                to: Address::ZERO,
                nullifier_hash: Field::from(2),
                relayer: Address::ZERO,
                fee: U256::from(1) << 200,
                block_number: block,
            }),
        ];
        let original: Snapshot = CoreSnapshot::new(pool.inner.clone(), block..block + 1, events)
            .unwrap()
            .into();

        let rebuilt = Snapshot::new(
            &pool,
            block.into(),
            (block + 1).into(),
            original.events().unwrap(),
        )
        .unwrap();
        assert_eq!(rebuilt.inner.events(), original.inner.events());
    }
}
