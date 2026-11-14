use std::hint::black_box;

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use kohaku_tornadocash::{Field, merkle_tree::MerkleTree};

const SIZES: [usize; 2] = [1_024, 32_768];
const REBUILD_SIZES: [usize; 2] = [64, 256];

fn leaves(n: usize) -> Vec<Field> {
    (0..n as u32).map(Field::from).collect()
}

fn bench_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("tornado_insert");
    for n in SIZES {
        let tree = MerkleTree::from_leaves(&leaves(n)).unwrap();
        group.bench_function(BenchmarkId::from_parameter(n), |b| {
            b.iter_batched(
                || tree.clone(),
                |mut tree| tree.insert(n, black_box(Field::from(u32::MAX))).unwrap(),
                BatchSize::LargeInput,
            );
        });
    }
    group.finish();
}

fn bench_proof(c: &mut Criterion) {
    let mut group = c.benchmark_group("tornado_proof");
    for n in SIZES {
        let tree = MerkleTree::from_leaves(&leaves(n)).unwrap();
        group.bench_function(BenchmarkId::from_parameter(n), |b| {
            b.iter(|| tree.proof(black_box(n / 2)).unwrap());
        });
    }
    group.finish();
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
    let mut group = c.benchmark_group("tornado_from_nodes");
    for n in SIZES {
        let nodes = MerkleTree::from_leaves(&leaves(n)).unwrap().into_nodes();
        group.bench_function(BenchmarkId::from_parameter(n), |b| {
            b.iter_batched(
                || nodes.clone(),
                |nodes| MerkleTree::from_nodes(nodes).unwrap(),
                BatchSize::LargeInput,
            );
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_insert,
    bench_proof,
    bench_rebuild,
    bench_from_nodes
);
criterion_main!(benches);
