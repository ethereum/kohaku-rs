#![allow(dead_code)]

use alloy::primitives::keccak256;
use kohaku_tornadocash::{
    note::{Nullifier, Secret},
    pool::Pool,
};
use kohaku_tornadocash_wallet::backend::{KeychainBackend, KeychainError};

/// Derives note material from `keccak256(label, nonce, pool_id)`.
///
/// Takes no secret input, so everything it produces is public. Fit only for tests.
pub struct TrivialKeychain;

#[async_trait::async_trait]
impl KeychainBackend for TrivialKeychain {
    async fn secrets(&self, pool: &Pool, nonce: u64) -> Result<(Secret, Nullifier), KeychainError> {
        Ok((
            Secret::new(derive(pool, nonce, 0)),
            Nullifier::new(derive(pool, nonce, 1)),
        ))
    }
}

fn derive(pool: &Pool, nonce: u64, label: u8) -> [u8; 31] {
    let mut preimage = vec![label];
    preimage.extend_from_slice(&nonce.to_be_bytes());
    preimage.extend_from_slice(pool.id().as_bytes());

    keccak256(preimage)[..31]
        .try_into()
        .expect("31 of 32 bytes")
}
