use rand::CryptoRng;
use websnark_rs::proof::{Proof, prove_random};

use crate::circuit::input::CircuitInputs;

pub mod input;
mod remote;

/// A tornadocash circuit, which can generate withdrawal proofs for known notes.
#[derive(Clone)]
pub struct Circuit {
    inner: Box<websnark_rs::circuit::Circuit>,
    proving_key: Box<websnark_rs::proving_key::ProvingKey>,
}

#[derive(Debug, thiserror::Error)]
pub enum CircuitError {
    #[error("Circuit error: {0}")]
    Circuit(#[from] websnark_rs::circuit::CircuitError),
    #[error("Proof generation error: {0}")]
    Proof(#[from] websnark_rs::proof::ProofError),
    #[error("Remote artifact loading error: {0}")]
    RemoteArtifact(Box<dyn std::error::Error + Send + Sync>),
}

impl Circuit {
    #[must_use]
    pub fn new(
        inner: websnark_rs::circuit::Circuit,
        proving_key: websnark_rs::proving_key::ProvingKey,
    ) -> Self {
        Self {
            inner: Box::new(inner),
            proving_key: Box::new(proving_key),
        }
    }

    /// Load the circuit from remote artifacts.
    ///
    /// # Errors
    /// Returns an error if the remote artifacts cannot be loaded.
    pub async fn from_remote() -> Result<Self, CircuitError> {
        let circuit = remote::load_remote_circuit()
            .await
            .map_err(CircuitError::RemoteArtifact)?;
        Ok(circuit)
    }

    /// Generates a proof for the given circuit inputs.
    ///
    /// # Errors
    /// Returns an error if the inputs are invalid or if the proof generation fails.
    pub fn prove(
        &self,
        inputs: &CircuitInputs,
        rng: &mut impl CryptoRng,
    ) -> Result<Proof, CircuitError> {
        let signals = inputs.as_signals();
        let witness = self.inner.witness(signals)?;
        let (proof, _) = prove_random(&self.proving_key, &witness, rng)?;
        Ok(proof)
    }
}
