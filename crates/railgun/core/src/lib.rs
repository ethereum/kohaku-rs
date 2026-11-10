#![doc = include_str!("../README.md")]

pub mod abis;
pub mod account;
pub mod builder;
pub mod caip;
pub mod chain_config;
pub mod circuit;
mod common;
pub mod crypto;
pub mod indexer;
pub mod merkle_tree;
pub use merkle_tree::{MerkleTreeState, MerkleTreeVerifier, TOTAL_LEAVES, UtxoMerkleTree};
pub mod note;
pub mod poi;
pub mod state;
pub mod wallet;
pub use state::RailgunCheckpoint;
pub mod transact;

#[cfg(all(test, native))]
mod interop_tests;

pub use common::MaybeSend;

#[cfg(all(wasm, parallel))]
compile_error!("The `parallel` feature is not supported in WASM builds.");

#[cfg(bench)]
pub mod bench_helpers {
    pub use crate::{indexer::indexed_account::*, note::*};
}
