use alloy::{providers::Provider, rpc::types::state::StateOverride, transports::TransportResult};

use crate::{
    bundler::PendingUserOperation,
    signable_user_operation::SignableUserOperation,
    signed_user_operation::SignedUserOperation,
    user_operation::{UserOperationGasEstimate, UserOperationHash, UserOperationReceipt},
};

/// ERC-4337 bundler methods (ERC-7769).
///
/// Only available on providers connected to a bundler.
#[cfg_attr(native, async_trait::async_trait)]
#[cfg_attr(wasm, async_trait::async_trait(?Send))]
pub trait BundlerApi: Send + Sync {
    async fn estimate_user_operation_gas(
        &self,
        op: &SignableUserOperation,
        state_override: Option<&StateOverride>,
    ) -> TransportResult<UserOperationGasEstimate>;

    async fn send_user_operation(
        &self,
        op: &SignedUserOperation,
    ) -> TransportResult<PendingUserOperation>;

    async fn get_user_operation_receipt(
        &self,
        hash: UserOperationHash,
    ) -> TransportResult<Option<UserOperationReceipt>>;
}

#[cfg_attr(native, async_trait::async_trait)]
#[cfg_attr(wasm, async_trait::async_trait(?Send))]
impl<P: Provider> BundlerApi for P {
    async fn estimate_user_operation_gas(
        &self,
        op: &SignableUserOperation,
        state_override: Option<&StateOverride>,
    ) -> TransportResult<UserOperationGasEstimate> {
        //? The state override is an optional trailing param, so omit it rather
        //? than sending `null`.
        let Some(state_override) = state_override else {
            return self
                .client()
                .request(
                    "eth_estimateUserOperationGas",
                    (op.user_op.clone(), op.entry_point),
                )
                .await;
        };

        self.client()
            .request(
                "eth_estimateUserOperationGas",
                (op.user_op.clone(), op.entry_point, state_override.clone()),
            )
            .await
    }

    async fn send_user_operation(
        &self,
        op: &SignedUserOperation,
    ) -> TransportResult<PendingUserOperation> {
        let hash = self
            .client()
            .request(
                "eth_sendUserOperation",
                (op.user_op.clone(), op.entry_point),
            )
            .await?;

        Ok(PendingUserOperation::new(self.root().clone(), hash))
    }

    async fn get_user_operation_receipt(
        &self,
        hash: UserOperationHash,
    ) -> TransportResult<Option<UserOperationReceipt>> {
        self.client()
            .request("eth_getUserOperationReceipt", (hash,))
            .await
    }
}
