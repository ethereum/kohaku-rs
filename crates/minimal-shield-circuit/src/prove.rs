use std::collections::HashMap;

use ark_bn254::{Bn254, Fq, Fq2, Fr, G1Affine, G2Affine};
use ark_circom::CircomReduction;
use ark_ff::PrimeField;
use ark_groth16::{Groth16, Proof as ArkProof, prepare_verifying_key};
use ark_relations::gr1cs::SynthesisError;
use num_bigint::{BigInt as NumBigInt, Sign};
use ruint::aliases::U256;
use thiserror::Error;
use wasmer::{Module, Store};

use crate::{
    artifacts::{ArtifactError, Artifacts, load_default},
    inputs::{CircuitInputs, NUM_PUBLIC_SIGNALS},
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
/// Returns the snarkjs-layout proof and the 3 circuit public signals in
/// verifier order: `beta, gamma, alpha`.
///
/// # Errors
/// Returns if artifacts are missing, witness calculation fails, or the proof does not verify.
pub fn prove(inputs: &CircuitInputs) -> Result<(Proof, [U256; NUM_PUBLIC_SIGNALS]), CircuitError> {
    prove_with(&load_default()?, inputs)
}

/// # Errors
/// Returns if witness calculation or proving fails.
pub fn prove_with(
    artifacts: &Artifacts,
    inputs: &CircuitInputs,
) -> Result<(Proof, [U256; NUM_PUBLIC_SIGNALS]), CircuitError> {
    let witnesses_fr = calculate_witness_fr(&artifacts.wasm, inputs.to_circuit_signals())?;
    let matrices = &artifacts.matrices;
    if matrices.num_instance_variables != NUM_PUBLIC_SIGNALS + 1 {
        return Err(CircuitError::Witness(format!(
            "expected {} instance variables, zkey has {}",
            NUM_PUBLIC_SIGNALS + 1,
            matrices.num_instance_variables
        )));
    }
    if witnesses_fr.len() < matrices.num_instance_variables {
        return Err(CircuitError::Witness(format!(
            "witness length {} < instance {}",
            witnesses_fr.len(),
            matrices.num_instance_variables
        )));
    }

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

    let mut publics = [U256::ZERO; NUM_PUBLIC_SIGNALS];
    for (dst, fr) in publics.iter_mut().zip(public_inputs.iter()) {
        *dst = fr.into_bigint().into();
    }
    Ok((proof.into(), publics))
}

/// Ark-verify a snarkjs-layout proof against converted artifacts.
///
/// # Errors
/// Returns if a coordinate is not a field element.
pub fn verify_snarkjs_proof(
    artifacts: &Artifacts,
    proof: &Proof,
    publics: &[U256],
) -> Result<bool, CircuitError> {
    verify_ark_proof(artifacts, &snarkjs_to_ark(proof)?, publics)
}

/// Ark-verify with G2 limbs left in ark `[c0, c1]` order (no snarkjs swap).
///
/// # Errors
/// Returns if a coordinate is not a field element.
pub fn verify_unswapped_g2_proof(
    artifacts: &Artifacts,
    proof: &Proof,
    publics: &[U256],
) -> Result<bool, CircuitError> {
    verify_ark_proof(artifacts, &unswapped_to_ark(proof)?, publics)
}

fn verify_ark_proof(
    artifacts: &Artifacts,
    proof: &ArkProof<Bn254>,
    publics: &[U256],
) -> Result<bool, CircuitError> {
    let mut frs = Vec::with_capacity(publics.len());
    for p in publics {
        let fr = Fr::from_bigint((*p).into())
            .ok_or_else(|| CircuitError::Witness(format!("public input {p} not in Fr")))?;
        frs.push(fr);
    }
    let pvk = prepare_verifying_key(&artifacts.proving_key.vk);
    Groth16::<Bn254, CircomReduction>::verify_proof(&pvk, proof, &frs).map_err(CircuitError::from)
}

fn snarkjs_to_ark(proof: &Proof) -> Result<ArkProof<Bn254>, CircuitError> {
    Ok(ArkProof {
        a: g1(proof.a)?,
        b: g2_snarkjs(proof.b)?,
        c: g1(proof.c)?,
    })
}

fn unswapped_to_ark(proof: &Proof) -> Result<ArkProof<Bn254>, CircuitError> {
    Ok(ArkProof {
        a: g1(proof.a)?,
        b: g2_ark(proof.b)?,
        c: g1(proof.c)?,
    })
}

fn u256_to_fq(v: U256) -> Result<Fq, CircuitError> {
    Fq::from_bigint(v.into()).ok_or_else(|| CircuitError::Witness(format!("coord {v} not in Fq")))
}

fn g1(xy: [U256; 2]) -> Result<G1Affine, CircuitError> {
    let p = G1Affine::new_unchecked(u256_to_fq(xy[0])?, u256_to_fq(xy[1])?);
    if !p.is_on_curve() {
        return Err(CircuitError::Witness("G1 not on curve".into()));
    }
    Ok(p)
}

fn g2_snarkjs(limbs: [[U256; 2]; 2]) -> Result<G2Affine, CircuitError> {
    // snarkjs soliditycalldata: each Fq2 is [c1, c0].
    g2(limbs[0][1], limbs[0][0], limbs[1][1], limbs[1][0])
}

fn g2_ark(limbs: [[U256; 2]; 2]) -> Result<G2Affine, CircuitError> {
    g2(limbs[0][0], limbs[0][1], limbs[1][0], limbs[1][1])
}

fn g2(x_c0: U256, x_c1: U256, y_c0: U256, y_c1: U256) -> Result<G2Affine, CircuitError> {
    let x = Fq2::new(u256_to_fq(x_c0)?, u256_to_fq(x_c1)?);
    let y = Fq2::new(u256_to_fq(y_c0)?, u256_to_fq(y_c1)?);
    let p = G2Affine::new_unchecked(x, y);
    if !p.is_on_curve() {
        return Err(CircuitError::Witness("G2 not on curve".into()));
    }
    Ok(p)
}

fn calculate_witness_fr(
    wasm: &[u8],
    inputs: HashMap<String, Vec<U256>>,
) -> Result<Vec<Fr>, CircuitError> {
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
    calculator
        .calculate_witness_element::<Fr, _>(&mut store, inputs, true)
        .map_err(|e| CircuitError::Witness(e.to_string()))
}
