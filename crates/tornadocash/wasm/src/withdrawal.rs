use kohaku_tornadocash::Withdrawal as CoreWithdrawal;
use rand::{
    SeedableRng,
    rngs::{StdRng, SysRng},
};
use tsify::Ts;
use wasm_bindgen::prelude::*;

use crate::{
    circuit_artifacts::CircuitArtifacts, hex::Hex, merkle_proof::MerkleProof, note::Note,
    payer::Payer, pool::Pool, proven_withdrawal::ProvenWithdrawal,
};

/// A Tornado Cash withdrawal awaiting a zero-knowledge proof.
///
/// See [`kohaku_tornadocash::Withdrawal`] for more details.
#[wasm_bindgen]
pub struct Withdrawal {
    pub(crate) inner: CoreWithdrawal,
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
            inner: CoreWithdrawal::new(&pool.inner, note.inner.clone(), Hex::decode(recipient)?),
        })
    }

    /// Serialize this withdrawal, including its note secrets, as a JSON string.
    ///
    /// The format is unspecified; only `fromJSON` from the same package version reads it.
    ///
    /// # Errors
    /// Throws if the withdrawal cannot be serialized.
    #[wasm_bindgen(js_name = toJSON)]
    pub fn to_json(&self) -> Result<String, JsError> {
        Ok(serde_json::to_string(&self.inner)?)
    }

    /// Parse a withdrawal from the output of `toJSON`.
    ///
    /// # Errors
    /// Throws if `json` is not a serialized withdrawal.
    #[wasm_bindgen(js_name = fromJSON)]
    pub fn from_json(json: &str) -> Result<Self, JsError> {
        Ok(Self {
            inner: serde_json::from_str(json)?,
        })
    }

    /// Set this withdrawal's payer.
    #[wasm_bindgen(js_name = withPayer)]
    pub fn with_payer(&mut self, payer: &Payer) {
        self.inner.payer = payer.inner;
    }

    /// Synchronously prove this withdrawal against the Merkle proof.
    ///
    /// # Errors
    /// Throws on system entropy failure, a nonzero native-asset refund, a mismatched
    /// proof leaf or a core proving error.
    pub fn prove(
        &self,
        #[wasm_bindgen(js_name = merkleProof)] merkle_proof: &MerkleProof,
        #[wasm_bindgen(js_name = circuitArtifacts)] circuit_artifacts: &CircuitArtifacts,
    ) -> Result<ProvenWithdrawal, JsError> {
        let mut rng = StdRng::try_from_rng(&mut SysRng).map_err(JsError::from)?;
        Ok(self
            .inner
            .clone()
            .prove(&merkle_proof.inner, &circuit_artifacts.inner, &mut rng)?
            .into())
    }
}
