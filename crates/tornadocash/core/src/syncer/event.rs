use std::ops::Range;

use alloy::{
    primitives::{Address, U256},
    rpc::types::Log,
    sol_types::SolEvent,
};
use serde::{Deserialize, Serialize};

use crate::{Pool, abis::tornado::Tornado, field::Field};

/// A set of events emitted by a pool within a given block range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    /// The pool these events belong to.
    pub pool: Pool,
    /// The half-open block range these events cover.
    pub range: Range<u64>,
    /// The events the pool emitted within `range`.
    ///
    /// Deposits must be contiguous and in ascending order by leaf index.
    pub events: Vec<SyncEvent>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncEvent {
    Deposit(Deposit),
    Withdrawal(Withdrawal),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deposit {
    pub commitment: Field,
    pub leaf_index: u32,
    pub block_number: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Withdrawal {
    pub to: Address,
    pub nullifier_hash: Field,
    pub relayer: Address,
    pub fee: U256,
    pub block_number: u64,
}

impl Snapshot {
    #[must_use]
    pub fn new(pool: Pool, range: Range<u64>, events: Vec<SyncEvent>) -> Self {
        Self {
            pool,
            range,
            events,
        }
    }
}

impl SyncEvent {
    /// Attempts to decode an alloy RPC log into a `SyncEvent`. Returns `None` if the log is not a
    /// known event.
    pub fn try_from_log(log: &Log) -> Option<Self> {
        match log.topics().first() {
            Some(&Tornado::Deposit::SIGNATURE_HASH) => {
                let decoded = Tornado::Deposit::decode_log(&log.inner).ok()?.data;
                Some(SyncEvent::Deposit(Deposit {
                    commitment: decoded.commitment.try_into().ok()?,
                    leaf_index: decoded.leafIndex,
                    block_number: log.block_number?,
                }))
            }
            Some(&Tornado::Withdrawal::SIGNATURE_HASH) => {
                let decoded = Tornado::Withdrawal::decode_log(&log.inner).ok()?.data;
                Some(SyncEvent::Withdrawal(Withdrawal {
                    to: decoded.to,
                    nullifier_hash: decoded.nullifierHash.try_into().ok()?,
                    relayer: decoded.relayer,
                    fee: decoded.fee,
                    block_number: log.block_number?,
                }))
            }
            _ => None,
        }
    }
}
