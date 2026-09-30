#![doc = include_str!("../README.md")]

use kohaku_tornadocash::{
    Deposit, Note, NoteString,
    note::{Nullifier, Secret},
    pool::Pool,
};
use ruint::aliases::U256;
use thiserror::Error;

mod dyn_keychain;
pub mod recovery;

pub use dyn_keychain::DynKeychain;

/// A deterministic keychain for tornadocash wallets.
#[async_trait::async_trait]
pub trait Keychain: Send + Sync {
    /// Gets the secret and nullifier for a given pool and nonce.
    ///
    /// The secret and nullifier must be derived deterministically from the pool and nonce.
    ///
    /// # Errors
    /// Returns an error if the material cannot be derived.
    async fn secrets(&self, pool: &Pool, nonce: u64) -> Result<(Secret, Nullifier), KeychainError>;

    /// Gets the deposit commitment for a given pool and nonce.
    ///
    /// # Errors
    /// Returns an error if the material cannot be derived.
    async fn commitment(&self, pool: &Pool, nonce: u64) -> Result<U256, KeychainError> {
        let (secret, nullifier) = self.secrets(pool, nonce).await?;
        Ok(Note::new(nullifier, secret).commitment())
    }

    /// Gets the nullifier hash for a given pool and nonce.
    ///
    /// # Errors
    /// Returns an error if the material cannot be derived.
    async fn nullifier_hash(&self, pool: &Pool, nonce: u64) -> Result<U256, KeychainError> {
        let (secret, nullifier) = self.secrets(pool, nonce).await?;
        Ok(Note::new(nullifier, secret).nullifier_hash())
    }

    /// Returns the full note for `pool` at `nonce`.
    ///
    /// # Errors
    /// Returns an error if the backend cannot derive the material.
    async fn note(&self, pool: &Pool, nonce: u64) -> Result<NoteString, KeychainError> {
        let (secret, nullifier) = self.secrets(pool, nonce).await?;
        Ok(NoteString::new(
            Note::new(nullifier, secret),
            pool.symbol(),
            pool.amount(),
            pool.chain_id,
        ))
    }

    /// Returns a deposit for `pool` at `nonce`.
    ///
    /// # Errors
    /// Returns an error if the backend cannot derive the material.
    async fn deposit(&self, pool: &Pool, nonce: u64) -> Result<Deposit, KeychainError> {
        let (secret, nullifier) = self.secrets(pool, nonce).await?;
        Ok(Deposit::new(pool, Note::new(nullifier, secret)))
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum KeychainError {
    #[error(transparent)]
    Other(Box<dyn std::error::Error + Send + Sync>),
}

impl KeychainError {
    pub fn other<E: std::error::Error + Send + Sync + 'static>(err: E) -> Self {
        Self::Other(Box::new(err))
    }
}
