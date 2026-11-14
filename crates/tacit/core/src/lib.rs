#![doc = include_str!("../README.md")]

mod abi;
mod address;
mod babyjubjub;
mod bech32m;
mod event;
mod field;
mod keys;
mod memo;
mod pool;
mod poseidon;
mod state;
mod statement;
mod sync;
mod time;
mod transaction;
mod tree;
mod wallet;

pub use abi::TacitPool;
#[cfg(feature = "mnemonic")]
pub use address::MnemonicError;
pub use address::{ADDRESS_HRP, Account, AddressError, DERIVATION_PURPOSE, TacitAddress};
pub use bech32m::Bech32Error;
pub use event::{LogDecodeError, TransactEvent};
pub use keys::{KeyError, VALUE_LIMIT};
pub use kohaku_tacit_circuit as circuit;
pub use memo::MEMO_LEN;
pub use pool::{Pool, RelayerConfig};
pub use state::{OwnedNote, SavedNote, SavedState, StateError, WalletState};
pub use statement::{ExtData, StatementError, pool_asset};
pub use sync::{SyncError, pool_head, root_size};
pub use time::sleep;
pub use transaction::{Call, Kind, ProvenTransaction, RelayTx, Transaction};
pub use tree::{IncTree, IncTreeState, TREE_DEPTH, TreeError};
pub use wallet::{Payer, SpendError, Wallet};

#[cfg(test)]
mod vectors;
