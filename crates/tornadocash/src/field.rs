use std::fmt::{Debug, Display};

use alloy::primitives::B256;
use ark_bn254::Fr;
use ruint::aliases::U256;
use serde::{Deserialize, Serialize};

/// Tornadocash BN254 field element.
#[derive(Copy, Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Field(Fr);

#[derive(Debug, thiserror::Error)]
#[error("Field not in range")]
pub struct NotInRangeError;

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
        Fr::from(value as u64).into()
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

impl Debug for Field {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "0x{self}")
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
        let b256: B256 = (*self).into();
        b256.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Field {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let b256 = B256::deserialize(deserializer)?;
        b256.try_into().map_err(serde::de::Error::custom)
    }
}
