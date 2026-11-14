//! Output memos: how a recipient finds and opens the notes paid to it.
//!
//! A memo is `E (33) ‖ sealed value (16) ‖ tag (16)`. `E = e·G` is a fresh secp256k1 key; the
//! shared secret `s = compressed(e·V)` derives the note's keys and the stream the value is sealed
//! with.

use alloy::primitives::keccak256;
use k256::{NonZeroScalar, ProjectivePoint, PublicKey, elliptic_curve::sec1::ToEncodedPoint};
use rand_core::CryptoRng;

use crate::{
    address::{Account, TacitAddress, secp_scalar},
    field::Fr,
    keys::{KeyError, check_value, leaf_of, output_keys, owned_keys},
};

/// The length of one output's memo.
pub const MEMO_LEN: usize = 65;

const TAG_AEAD: &[u8] = b"tacit-evm-pool-aead-v1";
const TAG_MAC: &[u8] = b"tacit-evm-pool-aead-tag-v1";

/// A new output note and the memo that lets its recipient find it.
#[derive(Clone)]
pub(crate) struct SealedNote {
    pub value: u128,
    pub npk: Fr,
    pub rho: Fr,
    #[cfg_attr(not(test), allow(dead_code))]
    pub leaf: Fr,
    pub memo: [u8; MEMO_LEN],
}

/// What opening a memo yields.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct OpenedNote {
    pub value: u128,
    pub rho: Fr,
    pub secret: [u8; 33],
}

fn stream_key(secret: &[u8; 33]) -> [u8; 32] {
    keccak256([TAG_AEAD, secret].concat()).0
}

fn mac(key: &[u8; 32], ct: &[u8]) -> [u8; 16] {
    let tag = keccak256([TAG_MAC, key, ct].concat());
    tag.0[..16].try_into().expect("16 bytes")
}

fn stream(key: &[u8; 32]) -> [u8; 16] {
    let block = keccak256([&key[..], &[0, 0]].concat());
    block.0[..16].try_into().expect("16 bytes")
}

fn shared_secret(point: ProjectivePoint) -> [u8; 33] {
    point
        .to_affine()
        .to_encoded_point(true)
        .as_bytes()
        .try_into()
        .expect("33 bytes")
}

/// A random non-zero secp256k1 scalar.
pub(crate) fn random_secp(rng: &mut impl CryptoRng) -> NonZeroScalar {
    loop {
        let mut bytes = [0u8; 32];
        rng.fill_bytes(&mut bytes);
        if let Some(x) = secp_scalar(&bytes) {
            return x;
        }
    }
}

/// An output of `value` for `to`, sealed with the ephemeral key `ephemeral`.
pub(crate) fn seal(
    to: &TacitAddress,
    value: u128,
    asset: Fr,
    ephemeral: &NonZeroScalar,
) -> Result<SealedNote, KeyError> {
    check_value(value)?;
    let secret = shared_secret(ProjectivePoint::from(to.view) * **ephemeral);
    let (npk, rho) = output_keys(to.spend, to.nullifier, &secret)?;
    let key = stream_key(&secret);
    let pad = stream(&key);
    let mut ct = value.to_be_bytes();
    for (c, p) in ct.iter_mut().zip(pad) {
        *c ^= p;
    }
    let mut memo = [0u8; MEMO_LEN];
    memo[..33].copy_from_slice(&shared_secret(ProjectivePoint::GENERATOR * **ephemeral));
    memo[33..49].copy_from_slice(&ct);
    memo[49..].copy_from_slice(&mac(&key, &ct));
    Ok(SealedNote {
        value,
        npk,
        rho,
        leaf: leaf_of(asset, value, npk, rho)?,
        memo,
    })
}

/// The note `memo` describes, if it is addressed to `account` and recomputes `leaf`.
///
/// Anything that is not a note of this account, malformed or not, is `None`.
pub(crate) fn open(account: &Account, memo: &[u8], leaf: Fr, asset: Fr) -> Option<OpenedNote> {
    if memo.len() != MEMO_LEN {
        return None;
    }
    let ephemeral = PublicKey::from_sec1_bytes(&memo[..33]).ok()?;
    let secret = shared_secret(ephemeral.to_projective() * *account.view);
    let key = stream_key(&secret);
    let ct = &memo[33..49];
    if mac(&key, ct) != memo[49..] {
        return None;
    }
    let pad = stream(&key);
    let mut plain = [0u8; 16];
    for (i, (c, p)) in ct.iter().zip(pad).enumerate() {
        plain[i] = c ^ p;
    }
    let value = u128::from_be_bytes(plain);
    let owned = owned_keys(&account.keys, &secret).ok()?;
    (leaf_of(asset, value, owned.npk, owned.rho).ok()? == leaf).then_some(OpenedNote {
        value,
        rho: owned.rho,
        secret,
    })
}
