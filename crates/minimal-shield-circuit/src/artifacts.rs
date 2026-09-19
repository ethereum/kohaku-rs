use std::{fs, io::Cursor, path::Path};

use ark_bn254::{Bn254, Fr};
use ark_circom::index::NPIndex;
use ark_groth16::ProvingKey;
use ark_serialize::CanonicalDeserialize;
use thiserror::Error;

use crate::matrices::SerializableNpIndex;

#[derive(Debug, Error)]
pub enum ArtifactError {
    #[error("circuit artifacts not found at {0}; run convert-msp-artifacts")]
    Missing(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("brotli: {0}")]
    Brotli(String),
    #[error("deserialize: {0}")]
    Deserialize(String),
}

pub struct Artifacts {
    pub wasm: Vec<u8>,
    pub proving_key: ProvingKey<Bn254>,
    pub matrices: NPIndex<Fr>,
}

fn decompress(bytes: &[u8]) -> Result<Vec<u8>, ArtifactError> {
    let mut out = Vec::new();
    brotli::BrotliDecompress(&mut Cursor::new(bytes), &mut out)
        .map_err(|e| ArtifactError::Brotli(e.to_string()))?;
    Ok(out)
}

/// Load converted artifacts from `dir` (`wasm.br`, `proving_key.bin.br`, `matrices.bin.br`).
///
/// # Errors
/// Returns if files are missing or cannot be decoded.
pub fn load_from_dir(dir: impl AsRef<Path>) -> Result<Artifacts, ArtifactError> {
    let dir = dir.as_ref();
    let wasm_path = dir.join("wasm.br");
    let pk_path = dir.join("proving_key.bin.br");
    let mat_path = dir.join("matrices.bin.br");
    if !wasm_path.exists() || !pk_path.exists() || !mat_path.exists() {
        return Err(ArtifactError::Missing(dir.display().to_string()));
    }
    let wasm = decompress(&fs::read(wasm_path)?)?;
    let pk_bytes = decompress(&fs::read(pk_path)?)?;
    let mat_bytes = decompress(&fs::read(mat_path)?)?;
    let proving_key = ProvingKey::<Bn254>::deserialize_uncompressed_unchecked(&pk_bytes[..])
        .map_err(|e| ArtifactError::Deserialize(e.to_string()))?;
    let matrices: SerializableNpIndex<Fr> =
        SerializableNpIndex::deserialize_uncompressed_unchecked(&mat_bytes[..])
            .map_err(|e| ArtifactError::Deserialize(e.to_string()))?;
    let matrices = matrices.into();
    Ok(Artifacts {
        wasm,
        proving_key,
        matrices,
    })
}

/// Default artifact directory: `MSP_CIRCUIT_ARTIFACTS` or `./artifacts`.
///
/// # Errors
/// Returns if artifacts cannot be loaded.
pub fn load_default() -> Result<Artifacts, ArtifactError> {
    let dir = std::env::var("MSP_CIRCUIT_ARTIFACTS").unwrap_or_else(|_| "artifacts".into());
    load_from_dir(dir)
}
