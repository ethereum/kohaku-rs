//! Conversions between the BN254 scalar field, the `BabyJubJub` scalar field and integers, and the
//! wide hashes keys are derived with.

use ark_ff::{BigInt, PrimeField};
use ruint::aliases::U256;
use sha2::{Digest, Sha256};

/// The BN254 scalar field: the circuit's field, and `BabyJubJub`'s base field.
pub(crate) type Fr = ark_bn254::Fr;

/// The prime-order subgroup of `BabyJubJub`: note spend and nullifier keys live here.
pub(crate) type Scalar = ark_ed_on_bn254::Fr;

/// `x` as an integer.
pub(crate) fn word(x: Fr) -> U256 {
    U256::from_limbs(x.into_bigint().0)
}

/// `x` as a field element, if it is one.
pub(crate) fn fr(x: U256) -> Option<Fr> {
    Fr::from_bigint(BigInt(x.into_limbs()))
}

/// `x mod p`.
pub(crate) fn fr_mod(x: U256) -> Fr {
    Fr::from_be_bytes_mod_order(&x.to_be_bytes::<32>())
}

pub(crate) fn scalar_word(x: Scalar) -> U256 {
    U256::from_limbs(x.into_bigint().0)
}

/// A `BabyJubJub` scalar as an element of the circuit's field (`L < p`).
pub(crate) fn scalar_to_fr(x: Scalar) -> Fr {
    Fr::from_bigint(BigInt(x.into_bigint().0)).expect("L < p")
}

/// A field element reduced into the `BabyJubJub` scalar field.
pub(crate) fn fr_to_scalar(x: Fr) -> Scalar {
    Scalar::from_be_bytes_mod_order(&word(x).to_be_bytes::<32>())
}

/// `SHA-256(tag ‖ parts ‖ 0x00) ‖ SHA-256(tag ‖ parts ‖ 0x01)`: 512 bits to reduce into a field.
pub(crate) fn wide(tag: &[u8], parts: &[&[u8]]) -> [u8; 64] {
    let block = |i: u8| {
        let mut hasher = Sha256::new();
        hasher.update(tag);
        for part in parts {
            hasher.update(part);
        }
        hasher.update([i]);
        <[u8; 32]>::from(hasher.finalize())
    };
    let mut out = [0u8; 64];
    out[..32].copy_from_slice(&block(0));
    out[32..].copy_from_slice(&block(1));
    out
}

/// A non-zero `BabyJubJub` scalar from [`wide`].
pub(crate) fn wide_scalar(tag: &[u8], parts: &[&[u8]]) -> Option<Scalar> {
    let x = Scalar::from_be_bytes_mod_order(&wide(tag, parts));
    (x != Scalar::from(0u8)).then_some(x)
}

/// A field element from [`wide`].
pub(crate) fn wide_fr(tag: &[u8], parts: &[&[u8]]) -> Fr {
    Fr::from_be_bytes_mod_order(&wide(tag, parts))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn babyjubjub_order_is_the_scalar_field() {
        let l = U256::from_str(
            "2736030358979909402780800718157159386076813972158567259200215660948447373041",
        )
        .unwrap();
        assert_eq!(U256::from_limbs(Scalar::MODULUS.0), l);
    }
}
