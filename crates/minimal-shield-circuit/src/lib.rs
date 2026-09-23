//! Groth16 / Circom 2 proving for the MSP `spend` circuit.

mod artifacts;
mod inputs;
pub mod matrices;
mod proof;
mod prove;

pub use artifacts::{ArtifactError, Artifacts, load_default, load_from_dir};
pub use inputs::{CircuitInputs, DEPTH, NUM_PUBLIC_SIGNALS};
pub use proof::Proof;
pub use prove::{CircuitError, prove, prove_with, verify_snarkjs_proof, verify_unswapped_g2_proof};
