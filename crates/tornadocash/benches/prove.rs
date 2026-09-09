use criterion::{Criterion, criterion_group, criterion_main};
use kohaku_tornadocash::circuit::{Circuit, input::CircuitInputs};

const SIGNALS_DATA: &str = include_str!("./signals.json");

fn bench_prove(c: &mut Criterion) {
    let inputs: CircuitInputs = serde_json::from_str(SIGNALS_DATA).unwrap();

    let rt = tokio::runtime::Runtime::new().unwrap();
    let circuit = rt.block_on(Circuit::from_remote()).unwrap();

    let mut rng = rand::rng();
    c.bench_function("generate_proof", |b| {
        b.iter(|| circuit.prove(&inputs, &mut rng));
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(10);
    targets = bench_prove
}
criterion_main!(benches);
