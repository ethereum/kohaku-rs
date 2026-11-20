use alloy::{
    primitives::{Address, Bytes, U256},
    rpc::types::TransactionRequest,
};

/// A contract call with a target, native value and encoded input.
#[derive(Debug, Clone)]
pub struct Call {
    pub target: Address,
    /// Native-currency amount to attach, in base units.
    pub value: U256,
    pub input: Bytes,
}

impl From<Call> for TransactionRequest {
    fn from(call: Call) -> Self {
        Self::default()
            .to(call.target)
            .value(call.value)
            .input(call.input.into())
    }
}
