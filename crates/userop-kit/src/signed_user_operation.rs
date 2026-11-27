use alloy::{dyn_abi::Eip712Domain, primitives::Address, sol_types::SolStruct};
use serde::{Deserialize, Serialize};

use crate::user_operation::{PackedUserOperation, UserOperation, UserOperationHash};

/// A signed 4337 UserOperation with an optional signed 7702 Authorization
/// ready to be sent to a bundler.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(js, derive(tsify::Tsify))]
#[cfg_attr(js, tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct SignedUserOperation {
    pub user_op: UserOperation,
    pub entry_point: Address,
    hash: UserOperationHash,
}

impl SignedUserOperation {
    /// Wraps a signed `user_op`, hashing it under its EntryPoint's EIP-712 `domain`.
    pub fn new(user_op: UserOperation, entry_point: Address, domain: &Eip712Domain) -> Self {
        let hash =
            UserOperationHash(PackedUserOperation::from(&user_op).eip712_signing_hash(domain));

        Self {
            user_op,
            entry_point,
            hash,
        }
    }

    /// The UserOperation hash, as the EntryPoint and bundler compute it.
    ///
    /// A bundler that already has the operation still returns this hash, so the caller can poll
    /// for the receipt instead of submitting it again.
    #[must_use]
    pub fn hash(&self) -> UserOperationHash {
        self.hash
    }
}
