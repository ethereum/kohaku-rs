//! Account initialization, signing domains, and execution encoding for ERC-4337 operations.

use alloy::{
    primitives::{Address, Bytes, U256, bytes},
    rpc::types::Authorization,
};
use alloy_sol_types::Eip712Domain;

pub mod simple_7702_smart_account;
pub mod simple_account;
pub mod simple_smart_account;

// A nonzero 65-byte signature used when estimating ECDSA account validation.
pub(crate) const DUMMY_SIGNATURE: Bytes = bytes!(
    "0xfffffffffffffffffffffffffffffff0000000000000000000000000000000007aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa1c"
);

/// Requires thread-safe shared access outside WebAssembly.
#[cfg(native)]
pub trait MaybeSend: Sync {}
#[cfg(native)]
impl<T: Sync + ?Sized> MaybeSend for T {}

/// Allows single-threaded account implementations in WebAssembly.
#[cfg(wasm)]
pub trait MaybeSend {}
#[cfg(wasm)]
impl<T: ?Sized> MaybeSend for T {}

/// Initialization required before an account can execute its UserOperation.
#[derive(Debug, Clone)]
pub enum AccountInit {
    /// Uses an existing account without deployment or delegation.
    None,
    /// Delegates the sender through EIP-7702.
    Eip7702(Authorization),
    /// Deploys the sender through a factory.
    Factory {
        /// Contract that deploys the account.
        factory: Address,
        /// Encoded factory call that creates the account.
        data: Bytes,
    },
}

/// Provides account initialization, nonce reads, and execution encoding.
#[cfg_attr(native, async_trait::async_trait)]
#[cfg_attr(wasm, async_trait::async_trait(?Send))]
pub trait SmartAccount: MaybeSend {
    /// Calls accepted by this account's execution ABI.
    type Call;

    /// Returns the UserOperation sender address.
    fn address(&self) -> Address;

    /// Returns the account's ERC-4337 EntryPoint address.
    fn entry_point(&self) -> Address;

    /// Returns the EIP-712 domain used to sign UserOperations.
    fn domain(&self) -> Eip712Domain;

    /// Reads the account's EntryPoint nonce.
    ///
    /// # Errors
    /// Returns an error if the nonce cannot be read or decoded.
    async fn nonce(&self) -> Result<U256, SmartAccountError>;

    /// Returns the account's deployment or delegation requirements.
    ///
    /// # Errors
    /// Returns an error if the account's initialization state cannot be read.
    async fn init(&self) -> Result<AccountInit, SmartAccountError>;

    /// Returns an estimation signature with the account's expected format.
    fn dummy_signature(&self) -> Bytes;

    /// Encodes calls using the account's execution ABI.
    fn abi_encode_call(call_data: &Self::Call) -> Bytes;
}

/// An account state read or decoding failure.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SmartAccountError {
    #[error("Provider error: {0}")]
    ProviderError(#[from] alloy::transports::RpcError<alloy::transports::TransportErrorKind>),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
