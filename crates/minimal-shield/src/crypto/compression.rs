//! Hybrid compression (eprint 2025/1500) for position-notes-v2.
//!
//! Statement order matches the circuit and dispatcher:
//! `[nf1, nf2, out_cm1, out_cm2, root, domain, public_amount, fee, recipient, authorizer]`.

use alloy::primitives::keccak256;
use ruint::aliases::U256;

use super::poseidon::{poseidon, P};

/// `keccak256(concat of 10×32-byte BE words) mod r`.
#[must_use]
pub fn compression_alpha(stmt: &[U256; 10]) -> U256 {
    let mut buf = [0u8; 320];
    for (i, x) in stmt.iter().enumerate() {
        buf[i * 32..(i + 1) * 32].copy_from_slice(&x.to_be_bytes::<32>());
    }
    U256::from_be_bytes(keccak256(buf).0) % P
}

/// `Poseidon(10)` of the statement (circuit-side hash / public signal β).
#[must_use]
pub fn compression_beta(stmt: &[U256; 10]) -> U256 {
    let inputs: [U256; 10] = std::array::from_fn(|i| stmt[i] % P);
    poseidon(&inputs)
}

/// Horner evaluation of the statement polynomial at `sigma` (γ when `σ = α+β`).
#[must_use]
pub fn fingerprint(sigma: U256, stmt: &[U256; 10]) -> U256 {
    let mut acc = U256::ZERO;
    for x in stmt.iter().rev() {
        acc = mul_mod(acc, sigma % P);
        acc = add_mod(acc, *x % P);
    }
    acc
}

fn add_mod(a: U256, b: U256) -> U256 {
    let (s, overflow) = a.overflowing_add(b);
    if overflow || s >= P {
        s.wrapping_sub(P)
    } else {
        s
    }
}

fn mul_mod(a: U256, b: U256) -> U256 {
    a.mul_mod(b, P)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_beta_gamma_roundtrip_shape() {
        let stmt = [
            U256::from(1u64),
            U256::from(2u64),
            U256::from(3u64),
            U256::from(4u64),
            U256::from(5u64),
            U256::from(6u64),
            U256::from(7u64),
            U256::from(8u64),
            U256::from(9u64),
            U256::from(10u64),
        ];
        let alpha = compression_alpha(&stmt);
        let beta = compression_beta(&stmt);
        let sigma = add_mod(alpha, beta);
        let gamma = fingerprint(sigma, &stmt);
        assert!(alpha < P);
        assert!(beta < P);
        assert!(gamma < P);
        assert_ne!(alpha, beta);
    }
}
