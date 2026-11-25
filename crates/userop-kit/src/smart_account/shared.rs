use alloy::{
    network::TransactionBuilder,
    primitives::{Address, Bytes, U256, aliases::U192},
    providers::Provider,
    rpc::types::TransactionRequest,
};
use alloy_sol_types::SolCall;
use anyhow::Context;

use crate::{
    abis::{base_account::BaseAccount, entry_point::EntryPoint},
    smart_account::{Call, SmartAccountError},
};

/// Reads `sender`'s key-0 nonce from `entry_point`.
pub async fn entry_point_nonce<P: Provider>(
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
pub fn encode_execute_batch(calls: &[Call]) -> Bytes {
    if calls.is_empty() {
        // If no calls, return empty data to save gas.
        return Bytes::new();
    }

    let calls = calls
        .iter()
        .map(|call| BaseAccount::Call {
            target: call.to,
            value: call.value,
            data: call.data.clone(),
        })
        .collect();

    BaseAccount::executeBatchCall::new((calls,))
        .abi_encode()
        .into()
}
