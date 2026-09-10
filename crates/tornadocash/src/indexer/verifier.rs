use std::sync::Arc;

use alloy::primitives::Address;
use ruint::aliases::U256;
use thiserror::Error;

use crate::provider::pool::Pool;

/// Generic verifier interface.
#[async_trait::async_trait]
pub trait VerifierBackend: Send + Sync {
    /// See [`Verifier::verify`].
    async fn verify(&self, pool: &Pool, root: U256) -> Result<(), VerifierError>;
}

/// A verifier for tornadocash.
///
/// Verifiers are used to verify the sync status of merkle trees against on-chain state.
#[derive(Clone)]
pub struct Verifier(Arc<dyn VerifierBackend>);

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum VerifierError {
    #[error("Invalid root: {root:?}")]
    InvalidRoot { root: U256 },
    #[error("Invalid contract {contract}: {reason}")]
    InvalidContract { contract: Address, reason: String },
    #[error(transparent)]
    Other(Box<dyn std::error::Error + Send + Sync>),
}

impl Verifier {
    pub fn new(verifier: impl VerifierBackend + 'static) -> Self {
        Self(Arc::new(verifier))
    }

    /// Verifies the given root against the on-chain state of the pool.
    ///
    /// # Errors
    /// Returns a [`VerifierError`] if the root is invalid or if there is an error
    /// communicating with the blockchain.
    pub async fn verify(&self, pool: &Pool, root: U256) -> Result<(), VerifierError> {
        self.0.verify(pool, root).await
    }
}

impl<T: VerifierBackend + 'static> From<T> for Verifier {
    fn from(verifier: T) -> Self {
        Self::new(verifier)
    }
}

impl VerifierError {
    pub fn other<E: std::error::Error + Send + Sync + 'static>(err: E) -> Self {
        Self::Other(Box::new(err))
    }
}
