use serde::{Deserialize, Serialize};
use websnark_rs::{circuit::Circuit, proving_key::ProvingKey};

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CircuitArtifactsError {
    #[error("Circuit artifacts decode error: {0}")]
    Decode(#[from] postcard::Error),
    #[error("Circuit artifacts have {0} unexpected trailing bytes")]
    TrailingBytes(usize),
}

/// The tornadocash-classic withdrawal circuit and its proving key.
#[derive(Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CircuitArtifacts {
    pub circuit: Circuit,
    pub proving_key: ProvingKey,
}

impl CircuitArtifacts {
    /// Decodes the artifacts from their postcard-encoded bytes.
    ///
    /// # Errors
    /// Returns an error if the bytes are not postcard-encoded artifacts, or have bytes left over.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CircuitArtifactsError> {
        let (artifacts, rest) = postcard::take_from_bytes(bytes)?;
        if !rest.is_empty() {
            return Err(CircuitArtifactsError::TrailingBytes(rest.len()));
        }
        Ok(artifacts)
    }
}

impl std::fmt::Debug for CircuitArtifacts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CircuitArtifacts").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_bytes_that_are_not_artifacts() {
        assert!(matches!(
            CircuitArtifacts::from_bytes(&[0xff; 8]),
            Err(CircuitArtifactsError::Decode(_))
        ));
    }
}
