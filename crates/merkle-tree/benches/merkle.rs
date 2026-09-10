use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use kohaku_kv_store::memory::MemoryStore;
use kohaku_merkle_tree::{MerkleTree, hasher::Hasher};
use rand::RngExt;
use ruint::aliases::U256;

struct BenchHasher;

impl Hasher for BenchHasher {
    fn hash(a: U256, b: U256) -> U256 {
        a ^ b.rotate_left(1)
    }

    fn zero() -> U256 {
        U256::ZERO
    }
}

type Tree = MerkleTree<20, BenchHasher>;

fn random_leaves(n: usize) -> Vec<U256> {
    let mut rng = rand::rng();
    (0..n)
        .map(|_| {
            let mut bytes = [0u8; 32];
            rng.fill(&mut bytes);
            U256::from_be_bytes(bytes)
        })
        .collect()
}

/// Benchmark the time taken to insert `n` leaves into the MerkleTree, for various values of `n`.
fn bench_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("merkle_insert");
    let rt = tokio::runtime::Runtime::new().unwrap();

    for n in [100, 10_000] {
        let leaves = random_leaves(n);

        group.bench_with_input(BenchmarkId::from_parameter(n), &leaves, |b, leaves| {
            b.to_async(&rt).iter(|| {
                let tree = Tree::new(MemoryStore::new().into());
                async move {
                    tree.splice(0, leaves)
                        .await
                        .expect("Failed to insert leaves");
                }
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_insert);
criterion_main!(benches);
