//! Groth16 proving and verification, the way snarkjs does both for a `.zkey`.
//!
//! snarkjs proving keys hold the `H` bases for the odd coset of the evaluation domain, so the
//! quotient is never divided out: `A·w` and `B·w` are evaluated on the domain, moved to the coset
//! by one inverse and one forward transform each, and `A·B − C` there is the scalar vector for `H`.

use ark_bn254::{Bn254, Fr, G1Affine, G1Projective, G2Affine, G2Projective};
use ark_ec::{CurveGroup, VariableBaseMSM, pairing::Pairing};
use ark_ff::{AdditiveGroup, BigInteger, Field, PrimeField};
use ark_poly::{EvaluationDomain, Radix2EvaluationDomain};
use sha2::{Digest, Sha256};

use crate::{CircuitError, Proof, zkey::ProvingKey};

/// Proves `witness` (the full assignment, `witness[0] = 1`) with blinding factors `r` and `s`.
pub(crate) fn prove(
    pk: &ProvingKey,
    witness: &[Fr],
    blind_r: Fr,
    blind_s: Fr,
) -> Result<Proof, CircuitError> {
    if witness.len() != pk.n_vars || witness[0] != Fr::ONE {
        return Err(CircuitError::WitnessProgram(
            "the witness does not fit the proving key".into(),
        ));
    }
    let quotient = quotient(pk, witness)?;
    let private = pk.n_public + 1;

    // A = α + Σ wᵢAᵢ + r·δ,  B = β + Σ wᵢBᵢ + s·δ (in G2 for the proof, in G1 for C)
    let proof_a = G1Projective::msm_unchecked(&pk.a, witness) + pk.alpha1 + pk.delta1 * blind_r;
    let proof_b = G2Projective::msm_unchecked(&pk.b2, witness) + pk.beta2 + pk.delta2 * blind_s;
    let b_g1 = G1Projective::msm_unchecked(&pk.b1, witness) + pk.beta1 + pk.delta1 * blind_s;
    // C = Σ wᵢCᵢ (private signals) + H + s·A + r·B − r·s·δ
    let proof_c = G1Projective::msm_unchecked(&pk.c, &witness[private..])
        + G1Projective::msm_unchecked(&pk.h, &quotient)
        + proof_a * blind_s
        + b_g1 * blind_r
        - pk.delta1 * (blind_r * blind_s);

    Ok(Proof {
        a: proof_a.into_affine(),
        b: proof_b.into_affine(),
        c: proof_c.into_affine(),
    })
}

/// `A·B − C` on the odd coset of the domain, for the `H` bases.
fn quotient(pk: &ProvingKey, witness: &[Fr]) -> Result<Vec<Fr>, CircuitError> {
    let n = pk.domain_size;
    let mut a = vec![Fr::ZERO; n];
    let mut b = vec![Fr::ZERO; n];
    for coefficient in &pk.coefficients {
        let term = coefficient.value * witness[coefficient.signal as usize];
        let row = if coefficient.b { &mut b } else { &mut a };
        row[coefficient.constraint as usize] += term;
    }
    let mut c: Vec<Fr> = a.iter().zip(&b).map(|(x, y)| *x * y).collect();

    let domain = Radix2EvaluationDomain::<Fr>::new(n).ok_or(CircuitError::ProvingKey("domain"))?;
    let shift = Radix2EvaluationDomain::<Fr>::new(2 * n)
        .ok_or(CircuitError::ProvingKey("domain"))?
        .group_gen();
    let coset = domain
        .get_coset(shift)
        .ok_or(CircuitError::ProvingKey("domain"))?;

    for values in [&mut a, &mut b, &mut c] {
        domain.ifft_in_place(values);
        coset.fft_in_place(values);
    }
    Ok(a.iter()
        .zip(&b)
        .zip(&c)
        .map(|((a, b), c)| *a * b - c)
        .collect())
}

/// Checks `proof` for `public` against the verification key held in the proving key.
pub(crate) fn verify(pk: &ProvingKey, proof: &Proof, public: &[Fr]) -> bool {
    if public.len() != pk.n_public {
        return false;
    }
    let inputs = pk.ic[0] + G1Projective::msm_unchecked(&pk.ic[1..], public);
    let lhs = [
        G1Affine::from(-G1Projective::from(proof.a)),
        pk.alpha1,
        inputs.into_affine(),
        proof.c,
    ];
    let rhs: [G2Affine; 4] = [proof.b, pk.beta2, pk.gamma2, pk.delta2];
    Bn254::multi_pairing(lhs, rhs).0 == <Bn254 as Pairing>::TargetField::ONE
}

/// SHA-256 over `alpha1 ‖ beta2 ‖ gamma2 ‖ delta2 ‖ IC`, each coordinate a 32-byte big-endian word
/// (`G2` coordinates as `c0, c1`): the content pin on the verification key, independent of its
/// file.
pub(crate) fn vk_hash(pk: &ProvingKey) -> [u8; 32] {
    let mut hasher = Sha256::new();
    let mut put = |x: &ark_bn254::Fq| hasher.update(x.into_bigint().to_bytes_be());
    put(&pk.alpha1.x);
    put(&pk.alpha1.y);
    for g in [&pk.beta2, &pk.gamma2, &pk.delta2] {
        put(&g.x.c0);
        put(&g.x.c1);
        put(&g.y.c0);
        put(&g.y.c1);
    }
    for point in &pk.ic {
        put(&point.x);
        put(&point.y);
    }
    hasher.finalize().into()
}
