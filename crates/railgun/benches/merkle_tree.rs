use std::hint::black_box;

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use kohaku_railgun::merkle_tree::MerkleTree;
use ruint::aliases::U256;

const SIZES: [usize; 2] = [1_024, 32_768];
const REBUILD_SIZES: [usize; 2] = [64, 256];

fn leaves(n: usize) -> Vec<U256> {
    (0..n).map(U256::from).collect()
}

fn tree(n: usize) -> MerkleTree {
    let mut tree = MerkleTree::new(0);
    tree.insert_leaves(&leaves(n), 0).unwrap();
    tree
}

fn bench_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("railgun_insert");
    for n in SIZES {
        let tree = tree(n);
        group.bench_function(BenchmarkId::from_parameter(n), |b| {
            b.iter_batched(
                || tree.clone(),
                |mut tree| tree.insert_leaves(&[black_box(U256::MAX)], n).unwrap(),
                BatchSize::LargeInput,
            );
        });
    }
    group.finish();
}

fn bench_proof(c: &mut Criterion) {
    let mut group = c.benchmark_group("railgun_proof");
    for n in SIZES {
        let tree = tree(n);
        group.bench_function(BenchmarkId::from_parameter(n), |b| {
            b.iter(|| tree.generate_proof_at(black_box(n / 2)).unwrap());
        });
    }
    group.finish();
}

fn bench_rebuild(c: &mut Criterion) {
    let mut group = c.benchmark_group("railgun_rebuild");
    for n in REBUILD_SIZES {
        let leaves = leaves(n);
        group.bench_function(BenchmarkId::from_parameter(n), |b| {
            b.iter(|| {
                let mut tree = MerkleTree::new(0);
                tree.insert_leaves(black_box(&leaves), 0).unwrap();
                tree
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_insert, bench_proof, bench_rebuild);
criterion_main!(benches);
