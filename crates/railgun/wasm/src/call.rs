use alloy::{
    primitives::{TxKind, U256},
    rpc::types::TransactionRequest,
};
use serde::Serialize;
use tsify::Tsify;
use wasm_bindgen::prelude::*;

use crate::{bigint::JsU128, hex::Hex};

/// Transaction fields prepared by the core. The caller supplies gas, nonce, and sender.
#[derive(Serialize, Tsify)]
pub struct Call {
    to: Hex,
    data: Hex,
    #[serde(with = "tsify::serde_wasm_bindgen::preserve")]
    #[tsify(type = "bigint")]
    value: JsU128,
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "Call[]")]
    pub type Calls;
}

impl TryFrom<TransactionRequest> for Call {
    type Error = JsError;

    fn try_from(tx: TransactionRequest) -> Result<Self, Self::Error> {
        let Some(TxKind::Call(to)) = tx.to else {
            return Err(JsError::new("shield transaction has no contract recipient"));
        };
        let Some(data) = tx.input.input() else {
            return Err(JsError::new("shield transaction has no calldata"));
        };
        let value = tx.value.unwrap_or(U256::ZERO);
        let value = u128::try_from(value)
            .map_err(|_| JsError::new("shield transaction value exceeds u128"))?;
        Ok(Self {
            to: to.into(),
            data: Hex::from(data.as_ref()),
            value: value.into(),
        })
    }
}
