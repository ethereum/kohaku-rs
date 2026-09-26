use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use kohaku_merkle_tree::{MerkleTree, hasher::Hasher};
use rand::RngExt;

#[derive(Copy, Clone)]
struct BenchHasher;

/// Rounds of mixing, chosen so hashing dominates tree construction the way a real algebraic hash
/// (e.g. MiMC) does.
const ROUNDS: u64 = 1024;

impl Hasher<2, u64> for BenchHasher {
    fn hash(children: [u64; 2]) -> u64 {
        let mut state = children[0] ^ children[1].rotate_left(1);
        for i in 0..ROUNDS {
            state = state.wrapping_mul(0x9E37_79B9_7F4A_7C15).rotate_left(29) ^ i;
        }
        state
    }

    fn zero() -> u64 {
        0
    }
}

type Tree = MerkleTree<20, 2, u64, BenchHasher>;

fn random_leaves(n: usize) -> Vec<u64> {
    let mut rng = rand::rng();
    (0..n).map(|_| rng.random()).collect()
}

fn bench_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("merkle_insert");

    for n in [100, 10_000] {
        let leaves = random_leaves(n);

        group.bench_function(BenchmarkId::from_parameter(n), |b| {
            b.iter(|| {
                let mut tree = Tree::new();
                black_box(tree.splice(0, &leaves).expect("Failed to insert leaves"));
            });
        });
    }

    group.finish();
}

fn bench_proof(c: &mut Criterion) {
    let mut group = c.benchmark_group("merkle_proof");

    for n in [100, 10_000] {
        let leaves = random_leaves(n);
        let mut tree = Tree::new();
        tree.splice(0, &leaves).expect("Failed to insert leaves");

        group.bench_function(BenchmarkId::from_parameter(n), |b| {
            b.iter(|| {
                for i in 0..n {
                    black_box(tree.proof(i).expect("Failed to generate proof"));
                }
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_insert, bench_proof);
criterion_main!(benches);
