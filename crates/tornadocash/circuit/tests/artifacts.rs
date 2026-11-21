//! Decoding the circuit artifacts kept in the repository for tests.

use kohaku_tornadocash_circuit::{CircuitArtifacts, CircuitArtifactsError};

const ARTIFACTS_BR: &[u8] = include_bytes!("../data/artifacts.bin.br");

// Decoding the 26 MB artifacts takes about 45 s in a debug build.
#[test]
#[ignore = "run with `cargo test --release -- --ignored`"]
fn rejects_trailing_bytes() {
    let mut bytes = Vec::new();
    brotli::BrotliDecompress(&mut &ARTIFACTS_BR[..], &mut bytes).unwrap();
    bytes.push(0);
    assert!(matches!(
        CircuitArtifacts::from_bytes(&bytes),
        Err(CircuitArtifactsError::TrailingBytes(1))
    ));
}
