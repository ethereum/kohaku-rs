use kohaku_tornadocash::{
    note::{Nullifier, Secret},
    pool::Pool,
};
use thiserror::Error;

/// Deterministic keychain backend for tornadocash wallet.
///
/// See [`crate::Keychain`] for more details.
#[async_trait::async_trait]
pub trait KeychainBackend: Send + Sync {
    /// Gets the secret and nullifier for a given pool and nonce.
    ///
    /// # Errors
    /// Returns an error if the material cannot be derived.
    async fn secrets(&self, pool: &Pool, nonce: u64) -> Result<(Secret, Nullifier), KeychainError>;
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
