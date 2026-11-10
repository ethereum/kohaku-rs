use alloy::primitives::Address;
use kohaku_tornadocash::Withdrawal as CoreWithdrawal;
use rand::{
    SeedableRng,
    rngs::{StdRng, SysRng},
};
use tsify::Ts;
use wasm_bindgen::prelude::*;

use crate::{
    hex::Hex, merkle_proof::MerkleProof, note::Note, payer::Payer, pool::Pool,
    proven_withdrawal::ProvenWithdrawal,
};

/// A Tornado Cash withdrawal awaiting a zero-knowledge proof.
///
/// See [`kohaku_tornadocash::Withdrawal`] for more details.
#[wasm_bindgen]
pub struct Withdrawal {
    inner: CoreWithdrawal,
}

#[wasm_bindgen]
impl Withdrawal {
    /// Construct an independently owned withdrawal, borrowing the pool and note.
    ///
    /// # Errors
    /// Throws if recipient is not a 20-byte Hex address.
    #[wasm_bindgen(constructor)]
    pub fn new(pool: &Pool, note: &Note, recipient: &Ts<Hex>) -> Result<Self, JsError> {
        Ok(Self {
            inner: CoreWithdrawal::new(
                &pool.inner,
                note.inner.clone(),
                Address::try_from(recipient.to_rust()?)?,
            ),
        })
    }

    /// Consume this withdrawal and return its replacement with the borrowed payer.
    #[wasm_bindgen(js_name = withPayer)]
    #[must_use]
    pub fn with_payer(self, payer: &Payer) -> Self {
        Self {
            inner: self.inner.with_payer(payer.inner),
        }
    }

    /// Consume this withdrawal and synchronously prove it against the borrowed Merkle proof.
    ///
    /// Proving blocks the calling thread; callers choose whether to use a worker.
    ///
    /// # Errors
    /// Throws on system entropy failure, a nonzero native-asset refund, a mismatched
    /// proof leaf or a core proving error.
    /// This withdrawal is consumed even when proving throws; the Merkle proof remains usable.
    pub fn prove(self, merkle_proof: &MerkleProof) -> Result<ProvenWithdrawal, JsError> {
        let mut rng = StdRng::try_from_rng(&mut SysRng).map_err(JsError::from)?;
        Ok(self.inner.prove(&merkle_proof.inner, &mut rng)?.into())
    }
}
