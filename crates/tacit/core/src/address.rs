//! Pool addresses and the account a single identity key opens.

use std::{fmt, str::FromStr};

use hmac::{Hmac, Mac};
use k256::{
    AffinePoint, NonZeroScalar, ProjectivePoint, PublicKey,
    elliptic_curve::{PrimeField as _, sec1::ToEncodedPoint},
};
use sha2::Sha256;

use crate::{
    babyjubjub::{Point, mul_base8},
    bech32m::{self, Bech32Error},
    keys::{KeyError, WalletKeys},
};

/// The human-readable part of a mainnet pool address.
pub const ADDRESS_HRP: &str = "bp";
const ADDRESS_LEN: usize = 97;
const NETWORK: &str = "mainnet";
const SEED_TAG: &[u8] = b"tacit-btc-pool-seed-v1";
const VIEW_TAG: &[u8] = b"tacit-btc-pool-wallet-view-v1";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AddressError {
    #[error(transparent)]
    Encoding(#[from] Bech32Error),
    #[error("unknown address prefix {0}")]
    Prefix(String),
    #[error("an address is {ADDRESS_LEN} bytes")]
    Length,
    #[error("invalid view key")]
    ViewKey,
    #[error("not a BabyJubJub subgroup point")]
    NotInSubgroup,
}

/// A pool address: what a sender needs to pay its owner.
///
/// It carries the secp256k1 view key `V` that output memos are encrypted to, and the `BabyJubJub`
/// points `A` and `N` that each note's spend and nullifier keys are tweaked from. The text form is
/// bech32m (`bp1…`) over `V (33, compressed) ‖ A (32, packed) ‖ N (32, packed)`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TacitAddress {
    pub(crate) view: AffinePoint,
    pub(crate) spend: Point,
    pub(crate) nullifier: Point,
}

impl TacitAddress {
    /// The address's 97 bytes.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; ADDRESS_LEN] {
        let mut out = [0u8; ADDRESS_LEN];
        out[..33].copy_from_slice(self.view.to_encoded_point(true).as_bytes());
        out[33..65].copy_from_slice(&self.spend.pack());
        out[65..].copy_from_slice(&self.nullifier.pack());
        out
    }

    /// Reads an address from its 97 bytes.
    ///
    /// # Errors
    /// Returns an error if a key is not a valid point of its group.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, AddressError> {
        if bytes.len() != ADDRESS_LEN {
            return Err(AddressError::Length);
        }
        let view = PublicKey::from_sec1_bytes(&bytes[..33])
            .map_err(|_| AddressError::ViewKey)?
            .as_affine()
            .to_owned();
        let point = |b: &[u8]| {
            let bytes: &[u8; 32] = b.try_into().map_err(|_| AddressError::Length)?;
            Point::unpack(bytes)
                .filter(|p| !p.is_identity())
                .ok_or(AddressError::NotInSubgroup)
        };
        Ok(Self {
            view,
            spend: point(&bytes[33..65])?,
            nullifier: point(&bytes[65..])?,
        })
    }
}

impl fmt::Display for TacitAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&bech32m::encode(ADDRESS_HRP, &self.to_bytes()))
    }
}

impl fmt::Debug for TacitAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TacitAddress({self})")
    }
}

impl FromStr for TacitAddress {
    type Err = AddressError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (hrp, bytes) = bech32m::decode(s.trim())?;
        if hrp != ADDRESS_HRP {
            return Err(AddressError::Prefix(hrp));
        }
        Self::from_bytes(&bytes)
    }
}

/// A wallet in the pool, derived from one 32-byte identity key.
///
/// Every Tacit app derives its keys the same way, so the same identity key opens the same balance
/// everywhere. Kohaku derives the identity key from the wallet's seed at `m/29793'/1'/{account}'`
/// (see [`Account::from_mnemonic`]).
#[derive(Clone)]
pub struct Account {
    pub(crate) view: NonZeroScalar,
    pub(crate) keys: WalletKeys,
    address: TacitAddress,
}

/// The BIP-32 purpose the identity key is derived under: `m/29793'/1'/{account}'`.
pub const DERIVATION_PURPOSE: u32 = 29793;

impl Account {
    /// The account an identity key opens.
    ///
    /// # Errors
    /// Returns an error if a derived scalar is zero (negligibly likely).
    pub fn from_identity(identity: &[u8; 32]) -> Result<Self, KeyError> {
        let mut mac = <Hmac<Sha256> as hmac::digest::KeyInit>::new(&pad_key(identity));
        mac.update(SEED_TAG);
        let seed: [u8; 32] = mac.finalize().into_bytes().into();

        let view = alloy::primitives::keccak256([VIEW_TAG, NETWORK.as_bytes(), &seed].concat());
        let view = reduce_secp(&view.0).ok_or(KeyError::ZeroScalar)?;

        let keys = WalletKeys::derive(&seed, NETWORK)?;
        let address = TacitAddress {
            view: (ProjectivePoint::GENERATOR * *view).to_affine(),
            spend: mul_base8(keys.a),
            nullifier: mul_base8(keys.n),
        };
        Ok(Self {
            view,
            keys,
            address,
        })
    }

    /// The account at `m/29793'/1'/{account}'` of a BIP-39 mnemonic, the identity key Kohaku's
    /// keystore gives the Tacit plugin.
    ///
    /// # Errors
    /// Returns an error if the mnemonic is invalid.
    #[cfg(feature = "mnemonic")]
    pub fn from_mnemonic(phrase: &str, account: u32) -> Result<Self, MnemonicError> {
        use alloy::signers::local::{MnemonicBuilder, coins_bip39::English};
        let signer = MnemonicBuilder::<English>::default()
            .phrase(phrase)
            .derivation_path(format!("m/{DERIVATION_PURPOSE}'/1'/{account}'"))?
            .build()?;
        Ok(Self::from_identity(&signer.to_bytes().0)?)
    }

    /// This account's address.
    #[must_use]
    pub fn address(&self) -> &TacitAddress {
        &self.address
    }
}

impl fmt::Debug for Account {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Account")
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "mnemonic")]
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MnemonicError {
    #[error(transparent)]
    Mnemonic(#[from] alloy::signers::local::LocalSignerError),
    #[error(transparent)]
    Key(#[from] KeyError),
}

/// An HMAC-SHA256 key shorter than the block, zero-padded to it, as HMAC itself does.
fn pad_key(key: &[u8; 32]) -> hmac::digest::Key<Hmac<Sha256>> {
    let mut block = hmac::digest::Key::<Hmac<Sha256>>::default();
    block[..32].copy_from_slice(key);
    block
}

/// `x mod n` for a 32-byte big-endian `x`, if non-zero.
pub(crate) fn reduce_secp(bytes: &[u8; 32]) -> Option<NonZeroScalar> {
    use k256::{U256, elliptic_curve::ops::Reduce};
    let x = <k256::Scalar as Reduce<U256>>::reduce(U256::from_be_slice(bytes));
    Option::from(NonZeroScalar::new(x))
}

/// A scalar from 32 big-endian bytes, if it is in range and non-zero.
pub(crate) fn secp_scalar(bytes: &[u8; 32]) -> Option<NonZeroScalar> {
    let x: Option<k256::Scalar> = k256::Scalar::from_repr((*bytes).into()).into();
    x.and_then(|x| NonZeroScalar::new(x).into())
}
