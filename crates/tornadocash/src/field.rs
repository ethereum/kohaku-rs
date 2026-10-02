use std::fmt::Display;

use alloy::primitives::B256;
use ark_bn254::Fr;
use ark_ff::PrimeField;
use ruint::aliases::U256;
use serde::{Deserialize, Serialize};

/// Tornadocash BN254 field element.
#[derive(Copy, Clone, Default, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Field(Fr);

#[derive(Debug, thiserror::Error)]
#[error("Not in range")]
pub struct NotInRangeError;

impl Field {
    /// Wrapping conversion from `U256` to `Field`.
    pub fn wrapping_from_u256(value: U256) -> Self {
        let fr = Fr::from_le_bytes_mod_order(value.as_le_slice());
        Field(fr)
    }

    /// Wrapping conversion from `B256` to `Field`.
    pub fn wrapping_from_b256(value: B256) -> Self {
        let u256: U256 = value.into();
        Self::wrapping_from_u256(u256)
    }
}

impl From<Field> for Fr {
    fn from(value: Field) -> Self {
        value.0
    }
}

impl From<Field> for U256 {
    fn from(value: Field) -> Self {
        value.0.into()
    }
}

impl From<Field> for B256 {
    fn from(value: Field) -> Self {
        let u256: U256 = value.into();
        u256.into()
    }
}

impl From<usize> for Field {
    fn from(value: usize) -> Self {
        (value as u64).into()
    }
}

impl From<u32> for Field {
    fn from(value: u32) -> Self {
        (value as u64).into()
    }
}

impl From<u64> for Field {
    fn from(value: u64) -> Self {
        Fr::from(value).into()
    }
}

impl From<Fr> for Field {
    fn from(value: Fr) -> Self {
        Field(value)
    }
}

impl TryFrom<U256> for Field {
    type Error = NotInRangeError;

    fn try_from(value: U256) -> Result<Self, Self::Error> {
        match value.try_into() {
            Ok(fr) => Ok(Field(fr)),
            Err(_) => Err(NotInRangeError),
        }
    }
}

impl TryFrom<B256> for Field {
    type Error = NotInRangeError;

    fn try_from(value: B256) -> Result<Self, Self::Error> {
        let u256: U256 = value.into();
        u256.try_into()
    }
}

impl Display for Field {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let u256: U256 = (*self).into();
        write!(f, "0x{}", hex::encode(u256.to_be_bytes::<32>()))
    }
}

impl Serialize for Field {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let u256: U256 = (*self).into();
        u256.to_be_bytes::<32>().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Field {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let bytes = <[u8; 32]>::deserialize(deserializer)?;
        Field::try_from(U256::from_be_bytes(bytes)).map_err(serde::de::Error::custom)
    }
}
