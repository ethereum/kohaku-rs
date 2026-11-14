use std::hint::black_box;

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use kohaku_railgun::merkle_tree::MerkleTree;
use ruint::aliases::U256;

const REBUILD_SIZES: [usize; 2] = [64, 256];

fn leaves(n: usize) -> Vec<U256> {
    (0..n).map(U256::from).collect()
}

fn bench_insert(c: &mut Criterion) {
    let mut tree = MerkleTree::new(0);
    c.bench_function("railgun_insert", |b| {
        b.iter(|| tree.insert_leaves(&[black_box(U256::from(1))], 0).unwrap());
    });
}

fn bench_proof(c: &mut Criterion) {
    let mut tree = MerkleTree::new(0);
    tree.insert_leaves(&leaves(16), 0).unwrap();
    c.bench_function("railgun_proof", |b| {
        b.iter(|| tree.generate_proof_at(black_box(0)).unwrap());
    });
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

fn bench_from_nodes(c: &mut Criterion) {
    let mut tree = MerkleTree::new(0);
    tree.insert_leaves(&leaves(16), 0).unwrap();
    let state = tree.state();
    c.bench_function("railgun_from_nodes", |b| {
        b.iter_batched(
            || state.clone(),
            |state| MerkleTree::from_state(state).unwrap(),
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
