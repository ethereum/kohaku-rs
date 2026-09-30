use std::sync::Arc;

use kohaku_tornadocash::{
    Deposit, Note, NoteString,
    note::{Nullifier, Secret},
    pool::Pool,
};
use thiserror::Error;

#[async_trait::async_trait]
impl Keychain for DynKeychain {
    async fn secrets(&self, pool: &Pool, nonce: u64) -> Result<(Secret, Nullifier), KeychainError> {
        self.0.secrets(pool, nonce).await
    }
}

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
}

pub trait KeychainExt: Keychain {
    /// Returns the full note for `pool` at `nonce`.
    ///
    /// # Errors
    /// Returns an error if the backend cannot derive the material.
    fn note(
        &self,
        pool: &Pool,
        nonce: u64,
    ) -> impl Future<Output = Result<NoteString, KeychainError>>;
    /// Returns a deposit for `pool` at `nonce`.
    ///
    /// # Errors
    /// Returns an error if the backend cannot derive the material.
    fn deposit(
        &self,
        pool: &Pool,
        nonce: u64,
    ) -> impl Future<Output = Result<Deposit, KeychainError>>;
}

#[derive(Clone)]
pub struct DynKeychain(Arc<dyn Keychain>);

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum KeychainError {
    #[error(transparent)]
    Other(Box<dyn std::error::Error + Send + Sync>),
}

impl<T: Keychain> KeychainExt for T {
    async fn note(&self, pool: &Pool, nonce: u64) -> Result<NoteString, KeychainError> {
        let (secret, nullifier) = self.secrets(pool, nonce).await?;
        Ok(NoteString::new(
            Note::new(nullifier, secret),
            pool.symbol(),
            pool.amount(),
            pool.chain_id,
        ))
    }
    async fn deposit(&self, pool: &Pool, nonce: u64) -> Result<Deposit, KeychainError> {
        let (secret, nullifier) = self.secrets(pool, nonce).await?;
        Ok(Deposit::new(pool, Note::new(nullifier, secret)))
    }
}

impl DynKeychain {
    pub fn new(backend: impl Keychain + 'static) -> Self {
        Self(Arc::new(backend))
    }
}

impl KeychainError {
    pub fn other<E: std::error::Error + Send + Sync + 'static>(err: E) -> Self {
        Self::Other(Box::new(err))
    }
}
