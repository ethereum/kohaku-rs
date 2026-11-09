use alloy::{
    primitives::{Address, Bytes, U256},
    rpc::types::Authorization,
};
use alloy_sol_types::Eip712Domain;

pub mod deployed_simple_account;
pub mod simple_7702_smart_account;
pub mod simple_smart_account;

/// Native async account methods can be shared across tasks; WASM stays single-threaded.
#[cfg(native)]
pub trait SmartAccountBounds: Sync {}
#[cfg(native)]
impl<T: Sync> SmartAccountBounds for T {}
#[cfg(wasm)]
pub trait SmartAccountBounds {}
#[cfg(wasm)]
impl<T> SmartAccountBounds for T {}

#[cfg_attr(native, async_trait::async_trait)]
#[cfg_attr(wasm, async_trait::async_trait(?Send))]
pub trait SmartAccount: SmartAccountBounds {
    type Call;

    /// Get the address of this smart account.
    fn address(&self) -> Address;

    /// 4337 EntryPoint address for this smart account.
    fn entry_point(&self) -> Address;

    /// EIP-712 domain for this smart account, which is used for signing UserOperations.
    fn domain(&self) -> Eip712Domain;

    /// 4337 nonce for this smart account.
    async fn nonce(&self) -> Result<U256, SmartAccountError>;

    /// EIP-7702 authorization for this smart account.
    async fn authorization(&self) -> Result<Authorization, SmartAccountError>;

    /// Validate sender reuse policy before building. Built-in accounts reject
    /// code/delegation, native/EntryPoint nonce or ETH unless explicitly opted in.
    async fn validate_sender(&self) -> Result<(), SmartAccountError> {
        Ok(())
    }

    /// Optional delegation authorization. Deployed accounts override this with `None`.
    async fn optional_authorization(&self) -> Result<Option<Authorization>, SmartAccountError> {
        self.authorization().await.map(Some)
    }

    /// Factory deployment for a counterfactual account. Omitted once deployed.
    async fn factory(&self) -> Result<Option<(Address, Bytes)>, SmartAccountError> {
        Ok(None)
    }

    /// Returns a dummy signature that can be used for gas estimation.
    fn dummy_signature(&self) -> Bytes;

    /// Encodes the provided call data into the format expected by this smart account's EntryPoint.
    fn abi_encode_call(call_data: &Self::Call) -> Bytes;
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SmartAccountError {
    #[error("Provider error: {0}")]
    ProviderError(#[from] alloy::transports::RpcError<alloy::transports::TransportErrorKind>),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// Shared preflight for the built-in delegated and deployed account profiles.
/// Cannot detect ERC20 holdings or past activity that leaves no current state.
pub async fn validate_unused_sender<P: alloy::providers::Provider + ?Sized>(
    provider: &P,
    sender: Address,
    entry_point: Address,
    allow_used: bool,
) -> Result<(), SmartAccountError> {
    if allow_used {
        return Ok(());
    }
    use alloy::{
        network::TransactionBuilder, primitives::aliases::U192, rpc::types::TransactionRequest,
    };
    use alloy_sol_types::SolCall;

    use crate::abis::entry_point::EntryPoint;
    let code = provider.get_code_at(sender).await?;
    let balance = provider.get_balance(sender).await?;
    let native_nonce = provider.get_transaction_count(sender).await?;
    let data = EntryPoint::getNonceCall::new((sender, U192::ZERO)).abi_encode();
    let ret = provider
        .call(
            TransactionRequest::default()
                .with_to(entry_point)
                .with_input(data),
        )
        .await?;
    let nonce = EntryPoint::getNonceCall::abi_decode_returns(&ret)
        .map_err(|e| SmartAccountError::Other(e.into()))?;
    if !code.is_empty() || !balance.is_zero() || native_nonce != 0 || !nonce.is_zero() {
        return Err(SmartAccountError::Other(anyhow::anyhow!(
            "Execution account {sender} appears touched (code/delegation, nonce or ETH); explicitly set allow_used(true) to use it"
        )));
    }
    Ok(())
}
