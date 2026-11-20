use kohaku_tornadocash_circuit::CircuitArtifacts as CoreCircuitArtifacts;
use wasm_bindgen::prelude::*;

/// The Tornado Cash withdrawal circuit and its proving key.
///
/// See [`kohaku_tornadocash_circuit::CircuitArtifacts`] for more details.
#[wasm_bindgen]
pub struct CircuitArtifacts {
    pub(crate) inner: CoreCircuitArtifacts,
}

#[wasm_bindgen]
impl CircuitArtifacts {
    /// Decode the postcard-encoded artifacts.
    ///
    /// # Errors
    /// Throws if the bytes are not encoded circuit artifacts.
    #[wasm_bindgen(js_name = fromBytes)]
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, JsError> {
        Ok(Self {
            inner: CoreCircuitArtifacts::from_bytes(bytes)?,
        })
    }
}

impl From<CoreCircuitArtifacts> for CircuitArtifacts {
    fn from(inner: CoreCircuitArtifacts) -> Self {
        Self { inner }
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
pub(crate) mod test_utils {
    use super::*;

    /// Read from the repository: the wasm32 tests only build from a repository checkout.
    const ARTIFACTS_BR: &[u8] = include_bytes!("../../circuit/data/artifacts.bin.br");

    pub fn artifacts() -> CircuitArtifacts {
        let mut bytes = Vec::new();
        brotli::BrotliDecompress(&mut &ARTIFACTS_BR[..], &mut bytes).unwrap();
        CircuitArtifacts::from_bytes(&bytes).unwrap()
    }
}
