//! Tornado Cash circuit artifacts for tests, read from the repository at compile time.

use std::sync::LazyLock;

use kohaku_tornadocash_circuit::CircuitArtifacts;

const ARTIFACTS_BR: &[u8] = include_bytes!("../../tornadocash/circuit/data/artifacts.bin.br");

static ARTIFACTS: LazyLock<CircuitArtifacts> = LazyLock::new(|| {
    let mut bytes = Vec::new();
    brotli::BrotliDecompress(&mut &ARTIFACTS_BR[..], &mut bytes)
        .expect("repository artifacts are valid brotli");
    CircuitArtifacts::from_bytes(&bytes).expect("repository artifacts decode")
});

/// Returns the Tornado Cash circuit artifacts, decoded once per test binary.
#[must_use]
pub fn tornado_artifacts() -> &'static CircuitArtifacts {
    &ARTIFACTS
}
