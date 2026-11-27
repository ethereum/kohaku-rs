use alloy::{
    primitives::{Address, Bytes, U256, aliases::U192},
    providers::Provider,
};
use alloy_sol_types::SolCall;

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
    Ok(EntryPoint::new(entry_point, provider)
        .getNonce(sender, U192::ZERO)
        .call()
        .await?)
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
