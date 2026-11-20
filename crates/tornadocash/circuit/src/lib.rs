#![doc = include_str!("../README.md")]

use rand::CryptoRng;
use websnark_rs::proof::{Proof, prove_random};

mod artifacts;
mod inputs;

pub use artifacts::{CircuitArtifacts, CircuitArtifactsError};
pub use inputs::CircuitInputs;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CircuitError {
    #[error("Circuit error: {0}")]
    Circuit(#[from] websnark_rs::circuit::CircuitError),
    #[error("Proof generation error: {0}")]
    Proof(#[from] websnark_rs::proof::ProofError),
    #[error("Field conversion error: {0}")]
    ToField(#[from] ruint::ToFieldError),
}

/// Generates a proof for the given circuit inputs.
///
/// # Errors
/// Returns an error if the inputs are invalid or if the proof generation fails.
pub fn prove(
    inputs: &CircuitInputs,
    circuit_artifacts: &CircuitArtifacts,
    rng: &mut impl CryptoRng,
) -> Result<Proof, CircuitError> {
    let signals = inputs.as_signals()?;
    let witness = circuit_artifacts.circuit.witness(signals)?;
    let (proof, _) = prove_random(&circuit_artifacts.proving_key, &witness, rng)?;
    Ok(proof)
}
