//! Signature-based keychain built.
//!
//! Uses the schema `nullifier = keccak256(b"nullifier" || pool.chain_id || pool.address ||
//! sig(domain, nonce))`. This way a single signature can be used to derive a nullifier / secret for
//! any pool, speeding up recovery.

use std::{collections::HashMap, sync::RwLock};

use alloy::{
    dyn_abi::Eip712Domain,
    primitives::{B256, keccak256},
    signers::{Signature, Signer},
    sol_types::eip712_domain,
};
use kohaku_tornadocash::{Field, Note, NoteString, Nullifier, Pool, Secret};

use crate::{Keychain, KeychainError};

mod sol {
    use alloy::sol;

    sol! {
        struct TornadoCashNote {
            uint64 nonce;
        }
    }
}

/// Signature-based keychain built on an alloy signer.
pub struct SignatureKeychain<S: Signer + Send + Sync> {
    signer: S,
    derived: RwLock<HashMap<(Pool, u64), Derived>>,
}

#[derive(Clone, Copy)]
struct Derived {
    commitment: Field,
    nullifier_hash: Field,
}

const DOMAIN: Eip712Domain = eip712_domain! {
    name: "TornadoCash Keychain",
    version: "1",
    // keccak256(b"tornado")
    salt: B256::new([
        0xc1, 0x12, 0x37, 0xb9, 0x77, 0x41, 0x8c, 0x70,
        0x5d, 0x2b, 0x06, 0xda, 0x70, 0x25, 0x66, 0xcb,
        0xfa, 0xb6, 0xec, 0xe8, 0xe4, 0x13, 0x93, 0x95,
        0xf0, 0x3c, 0x66, 0xa9, 0x18, 0x99, 0xaf, 0x6f
    ]),
};

impl<S: Signer + Send + Sync> SignatureKeychain<S> {
    pub fn new(signer: S) -> Self {
        Self {
            signer,
            derived: RwLock::new(HashMap::new()),
        }
    }

    /// Derives the commitment and nullifier hash for a given pool and nonce, caching
    /// the results for future calls.
    async fn derived(&self, pool: &Pool, nonce: u64) -> Result<Derived, KeychainError> {
        let key = (pool.clone(), nonce);
        if let Some(d) = self.derived.read().unwrap().get(&key).copied() {
            return Ok(d);
        }

        let note = self.note(pool, nonce).await?;
        let d = Derived {
            commitment: note.commitment(),
            nullifier_hash: note.nullifier_hash(),
        };

        self.derived.write().unwrap().insert(key, d);
        Ok(d)
    }
}

#[cfg_attr(native, async_trait::async_trait)]
#[cfg_attr(wasm, async_trait::async_trait(?Send))]
impl<S: Signer + Send + Sync> Keychain for SignatureKeychain<S> {
    async fn note(&self, pool: &Pool, nonce: u64) -> Result<NoteString, KeychainError> {
        let signature = self
            .signer
            .sign_typed_data(&sol::TornadoCashNote { nonce }, &DOMAIN)
            .await?;

        let secret = secret_from_signature(pool, &signature);
        let nullifier = nullifier_from_signature(pool, &signature);

        Ok(NoteString::from_pool(Note::new(nullifier, secret), pool))
    }

    async fn commitment(&self, pool: &Pool, nonce: u64) -> Result<Field, KeychainError> {
        Ok(self.derived(pool, nonce).await?.commitment)
    }

    async fn nullifier_hash(&self, pool: &Pool, nonce: u64) -> Result<Field, KeychainError> {
        Ok(self.derived(pool, nonce).await?.nullifier_hash)
    }
}

fn secret_from_signature(pool: &Pool, sig: &Signature) -> Secret {
    Secret::new(from_signature(b"secret", pool, sig))
}

fn nullifier_from_signature(pool: &Pool, sig: &Signature) -> Nullifier {
    Nullifier::new(from_signature(b"nullifier", pool, sig))
}

fn from_signature(domain: &[u8], pool: &Pool, sig: &Signature) -> [u8; 31] {
    let bytes = keccak256(
        [
            domain,
            pool.chain_id.to_le_bytes().as_slice(),
            pool.address.as_slice(),
            sig.as_bytes().as_slice(),
        ]
        .concat(),
    );
    let mut nullifier_bytes = [0u8; 31];
    nullifier_bytes.copy_from_slice(&bytes[..31]);

    nullifier_bytes
}

impl From<alloy::signers::Error> for KeychainError {
    fn from(err: alloy::signers::Error) -> Self {
        Self::other(err)
    }
}

#[cfg(test)]
mod tests {
    use alloy::{primitives::U256, signers::local::LocalSigner};

    use super::*;

    const POOL: Pool = Pool::ETHEREUM_ETHER_1;

    #[test]
    fn note_salt_is_tornado() {
        let expected = keccak256(b"tornado");
        assert_eq!(Some(expected), DOMAIN.salt);
    }

    #[tokio::test]
    async fn note_deterministic() {
        //? Hackery to get deterministic signer for testing.
        let signer = LocalSigner::from_bytes(&U256::from(42).into()).unwrap();
        let keychain = SignatureKeychain::new(signer);

        let note = keychain.note(&POOL, 0).await.unwrap();
        let expected = "tornado-eth-1-1-0x6e7b680a434a94f58ab3191a2db353a8b7de420c5d99d411392f1fc8b91e00547f137f1bce7ee67a7a46c3d4e884cf71fcd50dc0e5f1f238ed3f94e8d335";
        assert_eq!(expected, note.to_string());
    }

    #[tokio::test]
    async fn nonce_and_pool_different() {
        let signer = LocalSigner::from_bytes(&U256::from(42).into()).unwrap();
        let keychain = SignatureKeychain::new(signer);

        let note1 = keychain.note(&POOL, 0).await.unwrap();
        let note2 = keychain.note(&POOL, 1).await.unwrap();
        let note3 = keychain.note(&Pool::ETHEREUM_ETHER_10, 0).await.unwrap();

        assert_ne!(note1, note2);
        assert_ne!(note1, note3);
    }
}
