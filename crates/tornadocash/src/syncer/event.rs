use alloy::primitives::{Address, B256, U256};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncEvent {
    Deposit(Deposit),
    Withdrawal(Withdrawal),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deposit {
    pub commitment: B256,
    pub leaf_index: u32,
    pub block_number: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Withdrawal {
    pub to: Address,
    pub nullifier_hash: B256,
    pub relayer: Address,
    pub fee: U256,
    pub block_number: u64,
}

impl SyncEvent {
    pub fn new_deposit(commitment: B256, leaf_index: u32, block_number: u64) -> Self {
        Self::Deposit(Deposit {
            commitment,
            leaf_index,
            block_number,
        })
    }

    pub fn new_withdrawal(
        to: Address,
        nullifier_hash: B256,
        relayer: Address,
        fee: U256,
        block_number: u64,
    ) -> Self {
        Self::Withdrawal(Withdrawal {
            to,
            nullifier_hash,
            relayer,
            fee,
            block_number,
        })
    }
}
