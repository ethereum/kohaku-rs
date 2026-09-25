use std::sync::Arc;

use kohaku_tornadocash::{
    note::{Note, Nullifier, Secret},
    pool::Pool,
};

use crate::backend::{KeychainBackend, KeychainError};

/// A deterministic source of tornadocash note material.
///
/// Keychains MUST produce deterministic (secret, nullifier) for a given
/// (pool, nonce). This way many tornadocash notes can be derived from and
/// recovered by a single keychain.
#[derive(Clone)]
pub struct Keychain(Arc<dyn KeychainBackend>);

impl Keychain {
    pub fn new(backend: impl KeychainBackend + 'static) -> Self {
        Self(Arc::new(backend))
    }

    /// Returns the secret and nullifier for `pool` at `nonce`.
    ///
    /// # Errors
    /// Returns an error if the backend cannot derive the material.
    pub async fn secrets(
        &self,
        pool: &Pool,
        nonce: u64,
    ) -> Result<(Secret, Nullifier), KeychainError> {
        self.0.secrets(pool, nonce).await
    }

    /// Returns the full note for `pool` at `nonce`.
    ///
    /// # Errors
    /// Returns an error if the backend cannot derive the material.
    pub async fn note(&self, pool: &Pool, nonce: u64) -> Result<Note, KeychainError> {
        let (secret, nullifier) = self.secrets(pool, nonce).await?;
        Ok(Note::new(
            nullifier,
            secret,
            pool.symbol(),
            pool.amount(),
            pool.chain_id,
        ))
    }
}

impl<T: KeychainBackend + 'static> From<T> for Keychain {
    fn from(backend: T) -> Self {
        Self::new(backend)
    }
}
