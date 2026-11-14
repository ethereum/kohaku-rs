use alloy::{
    primitives::{B256, Bytes},
    rpc::types::Log,
    sol_types::SolEvent,
};
use ruint::aliases::U256;
use serde::{Deserialize, Serialize};

use crate::abi::TacitPool;

/// One `Transact` of the pool: the nullifiers it spent and the leaves it inserted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransactEvent {
    /// Zero for an empty input slot.
    pub nullifiers: [U256; 2],
    /// Zero for an empty output slot. A transaction with both outputs empty inserts nothing.
    pub out_leaves: [U256; 2],
    /// The index of `out_leaves[0]`.
    pub first_index: u64,
    pub memos: [Bytes; 2],
    pub block: u64,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LogDecodeError {
    #[error("not a Transact log")]
    NotTransact,
    #[error("log without a block number")]
    NoBlock,
    #[error("leaf index out of range")]
    Index,
    #[error(transparent)]
    Decode(#[from] alloy::sol_types::Error),
}

fn word(x: B256) -> U256 {
    U256::from_be_bytes(x.0)
}

impl TryFrom<&Log> for TransactEvent {
    type Error = LogDecodeError;

    fn try_from(log: &Log) -> Result<Self, Self::Error> {
        if log.topic0() != Some(&TacitPool::Transact::SIGNATURE_HASH) {
            return Err(LogDecodeError::NotTransact);
        }
        let event = TacitPool::Transact::decode_log(&log.inner)?;
        Ok(Self {
            nullifiers: [word(event.nf0), word(event.nf1)],
            out_leaves: [word(event.outLeaf0), word(event.outLeaf1)],
            first_index: u64::try_from(event.firstIndex).map_err(|_| LogDecodeError::Index)?,
            memos: [event.memo0.clone(), event.memo1.clone()],
            block: log.block_number.ok_or(LogDecodeError::NoBlock)?,
        })
    }
}
