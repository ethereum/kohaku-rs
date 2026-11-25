//! Account initialization, signing domains, and execution encoding for ERC-4337 operations.

use alloy::{
    primitives::{Address, Bytes, U256, bytes},
    rpc::types::Authorization,
};
use alloy_sol_types::Eip712Domain;
use serde::{Deserialize, Serialize};

mod shared;
mod simple_7702_account;
mod simple_account;

pub use simple_7702_account::{SIMPLE_7702_ACCOUNT_IMPLEMENTATION_V08, Simple7702Account};
pub use simple_account::SimpleAccount;

/// The conventional 65-byte stub signature for estimating ECDSA account validation.
pub const ECDSA_STUB_SIGNATURE: Bytes = bytes!(
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

/// A call executed by a Smart Account batch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(js, derive(tsify::Tsify))]
#[cfg_attr(js, tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct Call {
    #[cfg_attr(js, tsify(type = "`0x${string}`"))]
    pub to: Address,
    #[cfg_attr(js, tsify(type = "`0x${string}`"))]
    pub value: U256,
    #[cfg_attr(js, tsify(type = "`0x${string}`"))]
    pub data: Bytes,
}

/// Provides account initialization, nonce reads, and execution encoding.
#[cfg_attr(native, async_trait::async_trait)]
#[cfg_attr(wasm, async_trait::async_trait(?Send))]
pub trait SmartAccount: MaybeSend {
    /// One call in this account's execution ABI.
    type Call: Clone;

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
    fn stub_signature(&self) -> Bytes;

    /// Encodes `calls` using the account's execution ABI.
    fn encode_calls(calls: &[Self::Call]) -> Bytes;
}

/// An account state read or decoding failure.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SmartAccountError {
    #[error("Provider error: {0}")]
    ProviderError(#[from] alloy::transports::RpcError<alloy::transports::TransportErrorKind>),
    #[error("Contract call error: {0}")]
    ContractError(#[from] alloy::contract::Error),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
