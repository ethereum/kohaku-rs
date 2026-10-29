use alloy::primitives::Address;
use kohaku_tornadocash::Payer as CorePayer;
use tsify::{Ts, Tsify};
use wasm_bindgen::prelude::*;

use crate::{bigint::JsU256, hex::Hex};

/// The payer of a withdrawal transaction, with exact fee and refund amounts.
///
/// See [`kohaku_tornadocash::Payer`] for more details.
#[wasm_bindgen]
pub struct Payer {
    pub(crate) inner: CorePayer,
}

#[wasm_bindgen]
impl Payer {
    /// Construct an independently owned payer from its canonical fields.
    ///
    /// # Errors
    /// Throws if address is not a 20-byte Hex or either amount is not a bigint
    /// in `0..=U256::MAX`.
    #[wasm_bindgen(constructor)]
    pub fn new(address: &Ts<Hex>, fee: JsU256, refund: JsU256) -> Result<Self, JsError> {
        Ok(Self {
            inner: CorePayer {
                address: Address::try_from(address.to_rust()?)?,
                fee: fee.try_into()?,
                refund: refund.try_into()?,
            },
        })
    }

    /// Return the payer address as checksummed hex.
    #[wasm_bindgen(getter)]
    pub fn address(&self) -> Result<Ts<Hex>, JsError> {
        Ok(Hex::from(self.inner.address).into_ts()?)
    }

    /// Return the fee in the pool asset's base units.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn fee(&self) -> JsU256 {
        self.inner.fee.into()
    }

    /// Return the native-asset refund in base units.
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn refund(&self) -> JsU256 {
        self.inner.refund.into()
    }
}
