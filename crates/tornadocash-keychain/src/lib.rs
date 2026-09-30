#![doc = include_str!("../README.md")]

pub mod keychain;
pub mod recovery;

pub use keychain::{DynKeychain, Keychain, KeychainError, KeychainExt};
pub use recovery::{RecoveredNote, recover};
