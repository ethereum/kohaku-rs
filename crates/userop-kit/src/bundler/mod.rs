use alloy::rpc::types::state::StateOverride;

use crate::{
    signable_user_operation::SignableUserOperation,
    signed_user_operation::SignedUserOperation,
    user_operation::{UserOperationGasEstimate, UserOperationHash, UserOperationReceipt},
};

pub mod pimlico;
mod rpc_client;

/// A bundler provider for 4337 UserOperation JSON-RPC methods.
#[cfg_attr(native, async_trait::async_trait)]
#[cfg_attr(wasm, async_trait::async_trait(?Send))]
pub trait Bundler {
    async fn gas_price(&self) -> Result<Option<GasPrice>, BundlerError>;

    /// Estimates gas for `op`.
    ///
    /// Simulates against `state_override` if provided.
    async fn estimate_gas(
        &self,
        op: &SignableUserOperation,
        state_override: Option<&StateOverride>,
    ) -> Result<UserOperationGasEstimate, BundlerError>;
    async fn send_user_operation(
        &self,
        op: &SignedUserOperation,
    ) -> Result<UserOperationHash, BundlerError>;
    async fn receipt(
        &self,
        hash: UserOperationHash,
    ) -> Result<Option<UserOperationReceipt>, BundlerError>;
    async fn wait_for_receipt(
        &self,
        hash: UserOperationHash,
    ) -> Result<UserOperationReceipt, BundlerError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GasPrice {
    pub max_fee_per_gas: u128,
    pub max_priority_fee_per_gas: u128,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BundlerError {
    #[error("Timeout")]
    Timeout,
    #[error("Other: {0}")]
    Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}
