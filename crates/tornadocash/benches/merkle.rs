use std::sync::Arc;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use kohaku_tornadocash::{kv::MemoryKvStore, merkle_tree::tc::TcMerkleTree};
use rand::RngExt;
use ruint::aliases::U256;

fn random_leaves(n: usize) -> Vec<U256> {
    let mut rng = rand::rng();
    (0..n)
        .map(|_| {
            let mut bytes = [0u8; 32];
            rng.fill(&mut bytes);
            bytes[0] = 0; // clamp to 248 bits, well under the BN254 field size (~254 bits)
            U256::from_be_bytes(bytes)
        })
        .collect()
}

/// Benchmark the time taken to insert `n` leaves into the TornadoMerkleTree, for various values of
/// `n`.
fn bench_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("merkle_insert");
    let rt = tokio::runtime::Runtime::new().unwrap();

    for n in [100, 10_000] {
        let store = Arc::new(MemoryKvStore::default());
        let leaves = random_leaves(n);

        group.bench_with_input(BenchmarkId::from_parameter(n), &leaves, |b, leaves| {
            b.to_async(&rt).iter(|| {
                let tree = TcMerkleTree::new(store.clone());
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
