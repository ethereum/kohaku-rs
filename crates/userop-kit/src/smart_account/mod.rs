//! Account initialization, signing domains, and execution encoding for ERC-4337 operations.

use alloy::{
    network::TransactionBuilder,
    primitives::{Address, Bytes, U256, aliases::U192, bytes},
    providers::Provider,
    rpc::types::{Authorization, TransactionRequest},
};
use alloy_sol_types::{Eip712Domain, SolCall};
use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::abis::entry_point::EntryPoint;

pub mod simple_7702_smart_account;
pub mod simple_account;

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

/// A call executed by a Smart Account batch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(js, derive(tsify::Tsify))]
#[cfg_attr(js, tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct Call {
    #[cfg_attr(js, tsify(type = "`0x${string}`"))]
    pub target: Address,
    #[cfg_attr(js, tsify(type = "`0x${string}`"))]
    pub value: U256,
    #[cfg_attr(js, tsify(type = "`0x${string}`"))]
    pub data: Bytes,
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

/// Reads `sender`'s key-0 nonce from `entry_point`.
pub(crate) async fn entry_point_nonce<P: Provider>(
    provider: &P,
    entry_point: Address,
    sender: Address,
) -> Result<U256, SmartAccountError> {
    let data = EntryPoint::getNonceCall::new((sender, U192::ZERO)).abi_encode();
    let ret = provider
        .call(
            TransactionRequest::default()
                .with_to(entry_point)
                .with_input(data),
        )
        .await?;

    Ok(EntryPoint::getNonceCall::abi_decode_returns(&ret)
        .context("Invalid EntryPoint nonce response")?)
}

/// Encodes `calls` as `executeBatch(Call[])`, or empty calldata when there are none.
pub(crate) fn encode_execute_batch(calls: &[Call]) -> Bytes {
    if calls.is_empty() {
        // If no calls, return empty data to save gas.
        return Bytes::new();
    }

    let calls = calls
        .iter()
        .map(|call| abi::BaseAccount::Call {
            target: call.target,
            value: call.value,
            data: call.data.clone(),
        })
        .collect();

    abi::BaseAccount::executeBatchCall::new((calls,))
        .abi_encode()
        .into()
}

mod abi {
    use alloy::sol;

    sol!(
        contract BaseAccount {
            struct Call {
                address target;
                uint256 value;
                bytes data;
            }

            function executeBatch(Call[] calldata calls) external;
        }

    );
}
