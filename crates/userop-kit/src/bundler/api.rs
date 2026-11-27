use alloy::{
    providers::Provider,
    rpc::types::state::StateOverride,
    transports::{TransportError, TransportResult},
};

use crate::{
    bundler::{PendingUserOperation, fill::fill_user_operation},
    signable_user_operation::SignableUserOperation,
    signed_user_operation::SignedUserOperation,
    smart_account::{SmartAccount, SmartAccountError},
    user_operation::{UserOperationGasLimits, UserOperationHash, UserOperationReceipt},
    user_operation_request::UserOperationRequest,
};

/// ERC-4337 bundler methods (ERC-7769).
///
/// Only available on providers connected to a bundler.
#[cfg_attr(native, async_trait::async_trait)]
#[cfg_attr(wasm, async_trait::async_trait(?Send))]
pub trait BundlerApi: Send + Sync {
    /// Fills `request` from `account`, then estimates its gas limits if they are unset.
    ///
    /// With gas limits set, the bundler is never called, so any provider can prepare the operation.
    ///
    /// Estimation applies the request's state override and simulates with its fees.
    ///
    /// # Errors
    /// Returns an error if the account's state cannot be read or estimation fails.
    async fn prepare_user_operation<S: SmartAccount>(
        &self,
        account: &S,
        mut request: UserOperationRequest<S::Call>,
    ) -> Result<SignableUserOperation, PrepareUserOperationError> {
        let estimate = request.gas_limits.is_none();
        let state_override = request.state_override.take();
        let mut op = fill_user_operation(account, request).await?;
        if estimate {
            let gas_limits = self
                .estimate_user_operation_gas(&op, state_override.as_ref())
                .await?;
            op.user_op.set_gas_limits(gas_limits);
        }

        Ok(op)
    }

    async fn estimate_user_operation_gas(
        &self,
        op: &SignableUserOperation,
        state_override: Option<&StateOverride>,
    ) -> TransportResult<UserOperationGasLimits>;

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
    ) -> TransportResult<UserOperationGasLimits> {
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

/// A failure preparing a UserOperation.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PrepareUserOperationError {
    #[error("Smart account error: {0}")]
    SmartAccount(#[from] SmartAccountError),
    #[error("Transport error: {0}")]
    Transport(#[from] TransportError),
}
