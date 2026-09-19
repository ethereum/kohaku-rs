use std::collections::HashMap;

use ark_bn254::{Bn254, Fr};
use ark_circom::CircomReduction;
use ark_ff::BigInt;
use ark_groth16::{Groth16, prepare_verifying_key};
use ark_relations::gr1cs::SynthesisError;
use num_bigint::{BigInt as NumBigInt, Sign};
use ruint::aliases::U256;
use thiserror::Error;
use wasmer::{Module, Store};

use crate::{
    artifacts::{ArtifactError, Artifacts, load_default},
    inputs::CircuitInputs,
    proof::Proof,
};

#[derive(Debug, Error)]
pub enum CircuitError {
    #[error(transparent)]
    Artifacts(#[from] ArtifactError),
    #[error("witness: {0}")]
    Witness(String),
    #[error("wasmer: {0}")]
    Wasmer(String),
    #[error(transparent)]
    Synthesis(#[from] SynthesisError),
    #[error("proof verification failed")]
    InvalidProof,
}

/// Prove `inputs` and self-verify. Requires converted artifacts on disk.
///
/// # Errors
/// Returns if artifacts are missing, witness calculation fails, or the proof does not verify.
pub fn prove(inputs: &CircuitInputs) -> Result<Proof, CircuitError> {
    prove_with(&load_default()?, inputs)
}

/// # Errors
/// Returns if witness calculation or proving fails.
pub fn prove_with(artifacts: &Artifacts, inputs: &CircuitInputs) -> Result<Proof, CircuitError> {
    let witnesses = calculate_witness(&artifacts.wasm, inputs.to_circuit_signals())?;
    let witnesses_fr: Vec<Fr> = witnesses
        .iter()
        .map(|x| Fr::from(BigInt::from(*x)))
        .collect();

    let matrices = &artifacts.matrices;
    let proof = Groth16::<Bn254, CircomReduction>::create_proof_with_reduction_and_matrices(
        &artifacts.proving_key,
        ark_std::rand::random(),
        ark_std::rand::random(),
        &[matrices.a.clone(), matrices.b.clone()],
        matrices.num_instance_variables,
        matrices.num_constraints,
        &witnesses_fr,
    )?;

    let public_inputs = &witnesses_fr[1..matrices.num_instance_variables];
    let pvk = prepare_verifying_key(&artifacts.proving_key.vk);
    if !Groth16::<Bn254, CircomReduction>::verify_proof(&pvk, &proof, public_inputs)? {
        return Err(CircuitError::InvalidProof);
    }
    Ok(proof.into())
}

fn calculate_witness(
    wasm: &[u8],
    inputs: HashMap<String, Vec<U256>>,
) -> Result<Vec<U256>, CircuitError> {
    let mut store = Store::default();
    let module = Module::new(&store, wasm).map_err(|e| CircuitError::Wasmer(e.to_string()))?;
    let mut calculator = ark_circom::WitnessCalculator::from_module(&mut store, module)
        .map_err(|e| CircuitError::Witness(e.to_string()))?;
    let inputs: HashMap<String, Vec<NumBigInt>> = inputs
        .into_iter()
        .map(|(k, v)| {
            (
                k,
                v.into_iter()
                    .map(|x| NumBigInt::from_bytes_be(Sign::Plus, &x.to_be_bytes::<32>()))
                    .collect(),
            )
        })
        .collect();
    let witness = calculator
        .calculate_witness(&mut store, inputs, true)
        .map_err(|e| CircuitError::Witness(e.to_string()))?;
    Ok(witness
        .into_iter()
        .map(|w| {
            let (_, bytes) = w.to_bytes_be();
            U256::try_from_be_slice(&bytes).unwrap_or(U256::ZERO)
        })
        .collect())
}
