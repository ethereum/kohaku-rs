use alloy::primitives::Address;
use ruint::aliases::U256;
use thiserror::Error;

use crate::provider::pool::Pool;

/// A verifier for tornadocash that can verify a given root is valid for a given pool.
#[async_trait::async_trait]
pub trait Verifier: Send + Sync {
    async fn verify(&self, pool: &Pool, root: U256) -> Result<(), VerifierError>;
}

#[derive(Debug, Error)]
pub enum VerifierError {
    #[error("Invalid root: {root:?}")]
    InvalidRoot { root: U256 },
    #[error("Invalid contract {contract}: {reason}")]
    InvalidContract { contract: Address, reason: String },
    #[error(transparent)]
    Other(Box<dyn std::error::Error + Send + Sync>),
}

impl VerifierError {
    pub fn other<E: std::error::Error + Send + Sync + 'static>(err: E) -> Self {
        Self::Other(Box::new(err))
    }
}
