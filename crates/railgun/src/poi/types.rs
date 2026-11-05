//! Shared protocol types used by Railgun notes and optional POI extensions
//!
//! <https://github.com/Railgun-Community/private-proof-of-innocence/blob/4b1eaf6ef19099dbfd6b43b1ca78d2ce0132a752/packages/node/src/api/README.md>

use std::{fmt::Display, str::FromStr};

use ruint::aliases::U256;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ListKey(String);

#[derive(Debug, Copy, Clone, PartialEq, Deserialize, Eq, Hash)]
pub struct BlindedCommitment(U256);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TxidVersion {
    #[serde(rename = "V2_PoseidonMerkle")]
    V2PoseidonMerkle,
    #[serde(rename = "V3_PoseidonMerkle")]
    V3PoseidonMerkle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BlindedCommitmentType {
    Shield,
    Transact,
    Unshield,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PoiStatus {
    Valid,
    ProofSubmitted,
    Missing,
    ShieldBlocked,
    /// Local observation failure, not a status reported by a POI node.
    /// Does not establish absence and must not authorize proof submission.
    Unavailable,
}

impl Display for ListKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ListKey({})", self.0)
    }
}

impl Display for BlindedCommitment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BlindedCommitment({:#x})", self.0)
    }
}

impl From<String> for ListKey {
    fn from(value: String) -> Self {
        ListKey(value)
    }
}

impl From<ListKey> for String {
    fn from(value: ListKey) -> Self {
        value.0
    }
}

impl From<&str> for ListKey {
    fn from(value: &str) -> Self {
        ListKey(value.to_string())
    }
}

impl From<U256> for BlindedCommitment {
    fn from(value: U256) -> Self {
        BlindedCommitment(value)
    }
}

impl From<BlindedCommitment> for U256 {
    fn from(value: BlindedCommitment) -> Self {
        value.0
    }
}

impl Serialize for BlindedCommitment {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let hex_string = format!("0x{:064x}", self.0);
        serializer.serialize_str(&hex_string)
    }
}

impl Display for PoiStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            PoiStatus::Valid => "Valid",
            PoiStatus::ShieldBlocked => "ShieldBlocked",
            PoiStatus::ProofSubmitted => "ProofSubmitted",
            PoiStatus::Missing => "Missing",
            PoiStatus::Unavailable => "Unavailable",
        };
        write!(f, "{}", s)
    }
}

impl FromStr for PoiStatus {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "valid" => Ok(PoiStatus::Valid),
            "shieldblocked" => Ok(PoiStatus::ShieldBlocked),
            "proofsubmitted" => Ok(PoiStatus::ProofSubmitted),
            "missing" => Ok(PoiStatus::Missing),
            "unavailable" => Ok(PoiStatus::Unavailable),
            _ => Err(()),
        }
    }
}

impl FromStr for ListKey {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(ListKey(s.to_string()))
    }
}
