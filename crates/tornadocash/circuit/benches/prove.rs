use criterion::{Criterion, criterion_group, criterion_main};
use kohaku_tornadocash_circuit::{CircuitArtifacts, CircuitInputs, prove};

const SIGNALS_DATA: &str = include_str!("./signals.json");
const ARTIFACTS_BR: &[u8] = include_bytes!("../data/artifacts.bin.br");

fn bench_prove(c: &mut Criterion) {
    let inputs: CircuitInputs = serde_json::from_str(SIGNALS_DATA).unwrap();

    let mut bytes = Vec::new();
    brotli::BrotliDecompress(&mut &ARTIFACTS_BR[..], &mut bytes).unwrap();
    let artifacts = CircuitArtifacts::from_bytes(&bytes).unwrap();

    let mut rng = rand::rng();
    c.bench_function("generate_proof", |b| {
        b.iter(|| prove(&inputs, &artifacts, &mut rng));
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(10);
    targets = bench_prove
}
criterion_main!(benches);
