use std::hint::black_box;

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use kohaku_tornadocash::{Field, merkle_tree::MerkleTree};

const REBUILD_SIZES: [usize; 2] = [64, 256];

fn leaves(n: usize) -> Vec<Field> {
    (0..n as u32).map(Field::from).collect()
}

fn bench_insert(c: &mut Criterion) {
    let mut tree = MerkleTree::new();
    c.bench_function("tornado_insert", |b| {
        b.iter(|| tree.insert(0, black_box(Field::from(1))).unwrap());
    });
}

fn bench_proof(c: &mut Criterion) {
    let tree = MerkleTree::from_leaves(&leaves(16)).unwrap();
    c.bench_function("tornado_proof", |b| {
        b.iter(|| tree.proof(black_box(0)).unwrap());
    });
}

fn bench_rebuild(c: &mut Criterion) {
    let mut group = c.benchmark_group("tornado_rebuild");
    for n in REBUILD_SIZES {
        let leaves = leaves(n);
        group.bench_function(BenchmarkId::from_parameter(n), |b| {
            b.iter(|| MerkleTree::from_leaves(black_box(&leaves)).unwrap());
        });
    }
    group.finish();
}

fn bench_from_nodes(c: &mut Criterion) {
    let nodes = MerkleTree::from_leaves(&leaves(16)).unwrap().into_nodes();
    c.bench_function("tornado_from_nodes", |b| {
        b.iter_batched(
            || nodes.clone(),
            |nodes| MerkleTree::from_nodes(nodes).unwrap(),
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(
    benches,
    bench_insert,
    bench_proof,
    bench_rebuild,
    bench_from_nodes
);
criterion_main!(benches);
