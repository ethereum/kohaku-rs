//! Conversions at the JavaScript boundary: amounts as `bigint`, addresses and bytes as `0x` hex.

use alloy::primitives::Address;
use js_sys::{BigInt, Object, Reflect};
use kohaku_tacit::TacitAddress;
use wasm_bindgen::prelude::*;

/// A `bigint` as a non-negative amount below `2^128`.
pub(crate) fn amount(value: &BigInt) -> Result<u128, JsError> {
    let text = value
        .to_string(10)
        .map_err(|_| JsError::new("amount must be a bigint"))?;
    String::from(text)
        .parse()
        .map_err(|_| JsError::new("amount must be a non-negative bigint below 2^128"))
}

pub(crate) fn bigint(value: impl Into<u128>) -> BigInt {
    BigInt::from(value.into())
}

pub(crate) fn address(text: &str) -> Result<Address, JsError> {
    text.parse()
        .map_err(|_| JsError::new("not a 20-byte 0x address"))
}

pub(crate) fn pool_address(text: &str) -> Result<TacitAddress, JsError> {
    text.parse().map_err(JsError::from)
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}

/// A plain object from `(key, value)` pairs.
pub(crate) fn object(fields: &[(&str, JsValue)]) -> JsValue {
    let out = Object::new();
    for (key, value) in fields {
        let _ = Reflect::set(&out, &JsValue::from_str(key), value);
    }
    out.into()
}

pub(crate) fn error(error: impl std::error::Error) -> JsValue {
    JsError::from(error).into()
}
