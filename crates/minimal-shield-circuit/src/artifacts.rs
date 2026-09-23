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

impl Artifacts {
    /// `vk.alpha` as snarkjs Solidity `alphax` / `alphay`.
    #[must_use]
    pub fn alpha_g1(&self) -> (ruint::aliases::U256, ruint::aliases::U256) {
        use ark_ff::PrimeField;
        (
            self.proving_key.vk.alpha_g1.x.into_bigint().into(),
            self.proving_key.vk.alpha_g1.y.into_bigint().into(),
        )
    }

    /// `IC0` (constant term of `vk_x`) as snarkjs Solidity `IC0x` / `IC0y`.
    #[must_use]
    pub fn ic0(&self) -> (ruint::aliases::U256, ruint::aliases::U256) {
        use ark_ff::PrimeField;
        let p = &self.proving_key.vk.gamma_abc_g1[0];
        (p.x.into_bigint().into(), p.y.into_bigint().into())
    }

    /// `beta2.x` in snarkjs Solidity order (`betax1 = c1`, `betax2 = c0`).
    #[must_use]
    pub fn beta_g2_x_snarkjs(&self) -> (ruint::aliases::U256, ruint::aliases::U256) {
        use ark_ff::PrimeField;
        let x = &self.proving_key.vk.beta_g2.x;
        (x.c1.into_bigint().into(), x.c0.into_bigint().into())
    }
}
