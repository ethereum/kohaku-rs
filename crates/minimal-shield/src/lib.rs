//! Minimal shielded pool client for Hegota / EIP-8141 FrameTx.

pub mod abis;
pub mod crypto;
pub mod indexer;
pub mod merkle_tree;
pub mod note;
pub mod pool;
pub mod provider;
pub mod spend;

pub use note::Note;
pub use pool::Pool;
pub use provider::{Call, CreateAccount, PoolProvider, ProviderError};
