//! 8-field EIP-8141 FrameTx encoder for MSP HEAD / Hegota chain 8141.
//!
//! Wire: `0x06 || rlp([chain_id, nonce_keys, nonce_seq, sender, frames, signatures, fees, blob_hashes])`
//! Frame: `rlp([mode, flags, target, [execution, state], value, data])`

mod client;
mod gas;
mod recent_root;
mod rlp;
mod tx;

pub use client::{ClientError, FrameTxClient, SimulateFrame, SimulateResult};
pub use gas::*;
pub use recent_root::{
    recent_root_entry_hash, recent_root_storage_key, recent_root_tuple, recent_root_tuple_bytes,
    recent_root_window_error, source_id,
};
pub use tx::{Frame, FrameSig, FrameTx, FrameTxError};
