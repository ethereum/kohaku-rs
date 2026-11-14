//! Note keys, leaves, nullifiers and spend signatures.
//!
//! ```text
//! a, n      = hs_L("…-wallet-{spend,nk}-v1", network ‖ seed)        A = a·B8, N = n·B8
//! t_a, t_n  = hs_L("…-{auth,nk}-tweak-v1", s)                       s = 33-byte secp256k1 ECDH secret
//! rho       = hs_p("…-rho-v1", s)
//! Ak = A + t_a·B8    NK = N + t_n·B8    sk = a + t_a    nk = n + t_n   (mod L)
//! npk  = Poseidon(Ak.x, Ak.y, NK.x, NK.y)
//! leaf = Poseidon(asset, v, npk, rho)
//! nf   = Poseidon(nk, leaf, index)
//! ```

use ark_ff::{BigInteger, PrimeField};

use crate::{
    babyjubjub::{Point, mul_base8},
    field::{Fr, Scalar, fr_to_scalar, scalar_to_fr, wide_fr, wide_scalar},
    poseidon::poseidon,
};

const TAG_SPEND: &[u8] = b"tacit-btc-pool-zk-wallet-spend-v1";
const TAG_NK: &[u8] = b"tacit-btc-pool-zk-wallet-nk-v1";
const TAG_AUTH_TWEAK: &[u8] = b"tacit-btc-pool-zk-auth-tweak-v1";
const TAG_NK_TWEAK: &[u8] = b"tacit-btc-pool-zk-nk-tweak-v1";
const TAG_RHO: &[u8] = b"tacit-btc-pool-zk-rho-v1";
const TAG_NONCE: &[u8] = b"tacit-btc-pool-zk-eddsa-nonce-v1";

/// Note values are below `2^120` wei.
pub const VALUE_LIMIT: u128 = 1 << 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum KeyError {
    #[error("a derived scalar reduced to zero")]
    ZeroScalar,
    #[error("value must be below 2^120")]
    ValueOutOfRange,
    #[error("leaf index out of range")]
    IndexOutOfRange,
}

/// A wallet's `BabyJubJub` scalars: `a` authorizes spends, `n` derives nullifiers.
#[derive(Clone)]
pub(crate) struct WalletKeys {
    pub a: Scalar,
    pub n: Scalar,
}

impl WalletKeys {
    pub(crate) fn derive(seed: &[u8; 32], network: &str) -> Result<Self, KeyError> {
        let a = wide_scalar(TAG_SPEND, &[network.as_bytes(), seed]).ok_or(KeyError::ZeroScalar)?;
        let n = wide_scalar(TAG_NK, &[network.as_bytes(), seed]).ok_or(KeyError::ZeroScalar)?;
        Ok(Self { a, n })
    }
}

/// The keys of one owned note, from the wallet keys and the note's shared secret.
#[derive(Clone, Copy)]
pub(crate) struct NoteKeys {
    pub sk: Scalar,
    pub nk: Scalar,
    pub npk: Fr,
    pub rho: Fr,
}

struct Tweaks {
    auth: Scalar,
    nk: Scalar,
    rho: Fr,
}

fn tweaks(secret: &[u8; 33]) -> Result<Tweaks, KeyError> {
    Ok(Tweaks {
        auth: wide_scalar(TAG_AUTH_TWEAK, &[secret]).ok_or(KeyError::ZeroScalar)?,
        nk: wide_scalar(TAG_NK_TWEAK, &[secret]).ok_or(KeyError::ZeroScalar)?,
        rho: wide_fr(TAG_RHO, &[secret]),
    })
}

pub(crate) fn npk_of(ak: Point, nk: Point) -> Fr {
    poseidon(&[ak.x, ak.y, nk.x, nk.y])
}

/// Sender side: the recipient's note public key and `rho`, from its address points and the shared
/// secret.
pub(crate) fn output_keys(a: Point, n: Point, secret: &[u8; 33]) -> Result<(Fr, Fr), KeyError> {
    let t = tweaks(secret)?;
    let npk = npk_of(a.add(mul_base8(t.auth)), n.add(mul_base8(t.nk)));
    Ok((npk, t.rho))
}

/// Recipient side: the spend and nullifier scalars of the note `secret` opens.
pub(crate) fn owned_keys(wallet: &WalletKeys, secret: &[u8; 33]) -> Result<NoteKeys, KeyError> {
    let t = tweaks(secret)?;
    let sk = wallet.a + t.auth;
    let nk = wallet.n + t.nk;
    if sk == Scalar::from(0u8) || nk == Scalar::from(0u8) {
        return Err(KeyError::ZeroScalar);
    }
    Ok(NoteKeys {
        sk,
        nk,
        npk: npk_of(mul_base8(sk), mul_base8(nk)),
        rho: t.rho,
    })
}

pub(crate) fn check_value(value: u128) -> Result<Fr, KeyError> {
    if value >= VALUE_LIMIT {
        return Err(KeyError::ValueOutOfRange);
    }
    Ok(Fr::from(value))
}

pub(crate) fn leaf_of(asset: Fr, value: u128, npk: Fr, rho: Fr) -> Result<Fr, KeyError> {
    Ok(poseidon(&[asset, check_value(value)?, npk, rho]))
}

pub(crate) fn nullifier_of(nk: Scalar, leaf: Fr, index: u64) -> Result<Fr, KeyError> {
    if index >= 1 << 32 {
        return Err(KeyError::IndexOutOfRange);
    }
    Ok(poseidon(&[scalar_to_fr(nk), leaf, Fr::from(index)]))
}

/// An EdDSA-Poseidon signature over `BabyJubJub`, as circomlib verifies it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Signature {
    pub r8: Point,
    pub s: Scalar,
}

/// Signs `message` with `sk`. The nonce is derived from the key and the message.
pub(crate) fn sign(sk: Scalar, message: Fr) -> Signature {
    let a = mul_base8(sk);
    let be = |x: ark_ff::BigInt<4>| x.to_bytes_be();
    let r = wide_scalar(
        TAG_NONCE,
        &[&be(sk.into_bigint()), &be(message.into_bigint())],
    )
    .expect("a zero nonce has negligible probability");
    let r8 = mul_base8(r);
    let h = poseidon(&[r8.x, r8.y, a.x, a.y, message]);
    Signature {
        r8,
        s: r + Scalar::from(8u8) * fr_to_scalar(h) * sk,
    }
}
