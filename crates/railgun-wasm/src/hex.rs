use alloy::primitives::Address;
use serde::{Deserialize, Serialize};
use tsify::Tsify;

#[derive(Serialize, Deserialize, Tsify)]
#[tsify(type = "`0x${string}`")]
pub struct Hex(String);

#[derive(Debug, thiserror::Error)]
pub enum HexError {
    #[error("Hex must start with 0x")]
    MissingPrefix,
    #[error("Invalid hex: {0}")]
    InvalidHex(#[from] hex::FromHexError),
    #[error("Expected {expected} bytes, got {actual}")]
    InvalidLength { expected: usize, actual: usize },
}

impl Hex {
    pub fn bytes(&self) -> Result<Vec<u8>, HexError> {
        let text = self.0.strip_prefix("0x").ok_or(HexError::MissingPrefix)?;
        Ok(hex::decode(text)?)
    }

    pub fn fixed<const N: usize>(&self) -> Result<[u8; N], HexError> {
        let bytes = self.bytes()?;
        bytes
            .try_into()
            .map_err(|bytes: Vec<u8>| HexError::InvalidLength {
                expected: N,
                actual: bytes.len(),
            })
    }
}

impl TryFrom<Hex> for Address {
    type Error = HexError;

    fn try_from(value: Hex) -> Result<Self, Self::Error> {
        Ok(Self::from(value.fixed::<20>()?))
    }
}

impl From<Address> for Hex {
    fn from(value: Address) -> Self {
        Self(value.to_string())
    }
}

impl From<&[u8]> for Hex {
    fn from(value: &[u8]) -> Self {
        Self(format!("0x{}", hex::encode(value)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_inputs_require_prefix_and_exact_width() {
        assert!(matches!(
            Hex("00".repeat(32)).fixed::<32>(),
            Err(HexError::MissingPrefix)
        ));
        assert!(matches!(
            Hex(format!("0x{}", "00".repeat(31))).fixed::<32>(),
            Err(HexError::InvalidLength {
                expected: 32,
                actual: 31
            })
        ));
        assert_eq!(
            Hex(format!("0x{}", "ab".repeat(32))).fixed::<32>().unwrap(),
            [0xab; 32]
        );
    }
}
