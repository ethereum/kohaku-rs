//! Signature-based keychain built.

use alloy::{
    dyn_abi::Eip712Domain,
    primitives::keccak256,
    signers::{Signature, Signer},
    sol_types::eip712_domain,
};
use kohaku_tornadocash::{
    Note, NoteString,
    note::{Nullifier, Secret},
    pool::Pool,
};
use ruint::aliases::U256;

use crate::{Keychain, KeychainError};

mod sol {
    use alloy::sol;

    sol! {
        struct TornadoCashNote {
            string symbol;
            string amount;
            uint64 chainId;
            uint64 nonce;
        }
    }
}

/// Signature-based keychain built on an alloy signer.
pub struct SignatureKeychain<S: Signer + Send + Sync> {
    signer: S,
}

impl<S: Signer + Send + Sync> SignatureKeychain<S> {
    pub fn new(signer: S) -> Self {
        Self { signer }
    }
}

#[cfg_attr(native, async_trait::async_trait)]
#[cfg_attr(wasm, async_trait::async_trait(?Send))]
impl<S: Signer + Send + Sync> Keychain for SignatureKeychain<S> {
    async fn note(&self, pool: &Pool, nonce: u64) -> Result<NoteString, KeychainError> {
        let signature = self
            .signer
            .sign_typed_data(&payload(pool, nonce), &domain(pool))
            .await?;

        let secret = secret_from_signature(&signature);
        let nullifier = nullifier_from_signature(&signature);

        Ok(NoteString::from_pool(
            Note::new(nullifier, secret),
            pool.clone(),
        ))
    }

    async fn commitment(&self, pool: &Pool, nonce: u64) -> Result<U256, KeychainError> {
        let note = self.note(pool, nonce).await?;
        Ok(note.commitment().into())
    }

    async fn nullifier_hash(&self, pool: &Pool, nonce: u64) -> Result<U256, KeychainError> {
        let note = self.note(pool, nonce).await?;
        Ok(note.nullifier_hash().into())
    }
}

fn secret_from_signature(sig: &Signature) -> Secret {
    Secret::new(from_signature(b"secret", sig))
}

fn nullifier_from_signature(sig: &Signature) -> Nullifier {
    Nullifier::new(from_signature(b"nullifier", sig))
}

fn from_signature(domain: &[u8], sig: &Signature) -> [u8; 31] {
    let bytes = keccak256([domain, sig.as_bytes().as_slice()].concat());
    let mut nullifier_bytes = [0u8; 31];
    nullifier_bytes.copy_from_slice(&bytes[..31]);

    nullifier_bytes
}

fn payload(pool: &Pool, nonce: u64) -> sol::TornadoCashNote {
    sol::TornadoCashNote {
        symbol: pool.symbol().to_string(),
        amount: pool.amount().to_string(),
        chainId: pool.chain_id,
        nonce,
    }
}

fn domain(pool: &Pool) -> Eip712Domain {
    eip712_domain! {
        name: "TornadoCash Keychain",
        version: "1",
        chain_id: pool.chain_id,
        verifying_contract: pool.address,
        salt: keccak256("kohaku"),
    }
}

impl From<alloy::signers::Error> for KeychainError {
    fn from(err: alloy::signers::Error) -> Self {
        Self::other(err)
    }
}

#[cfg(test)]
mod tests {
    use alloy::signers::local::LocalSigner;

    use super::*;

    const POOL: Pool = Pool::ETHEREUM_ETHER_1;

    #[tokio::test]
    async fn signature_keychain_deterministic() {
        //? Hackery to get deterministic signer for testing.
        let signer = LocalSigner::from_bytes(&U256::from(42).into()).unwrap();
        let keychain = SignatureKeychain::new(signer);

        let note = keychain.note(&POOL, 0).await.unwrap();
        let expected = "tornado-eth-1-1-0x57097d3444b76cb0523228d4b26c805daf1af6fcebebf7564dd21a57ba7b37bcf98dcd3960d3519373dd127890f87205f8f42ba75c3c82cc13904b98d97a";
        assert_eq!(expected, note.to_string());
    }
}
