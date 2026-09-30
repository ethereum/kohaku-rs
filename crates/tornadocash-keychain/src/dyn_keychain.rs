use std::sync::Arc;

use kohaku_tornadocash::{
    note::{Nullifier, Secret},
    pool::Pool,
};
use ruint::aliases::U256;

use crate::{Keychain, KeychainError};

/// Dyn-compatible keychain wrapper.
#[derive(Clone)]
pub struct DynKeychain(Arc<dyn Keychain>);

impl DynKeychain {
    pub fn new(backend: impl Keychain + 'static) -> Self {
        Self(Arc::new(backend))
    }
}

#[async_trait::async_trait]
impl Keychain for DynKeychain {
    async fn secrets(&self, pool: &Pool, nonce: u64) -> Result<(Secret, Nullifier), KeychainError> {
        self.0.secrets(pool, nonce).await
    }
    async fn commitment(&self, pool: &Pool, nonce: u64) -> Result<U256, KeychainError> {
        self.0.commitment(pool, nonce).await
    }
    async fn nullifier_hash(&self, pool: &Pool, nonce: u64) -> Result<U256, KeychainError> {
        self.0.nullifier_hash(pool, nonce).await
    }
}
