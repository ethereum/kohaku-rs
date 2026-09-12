# kohaku-tornadocash

Rust [Tornadocash](https://tornadocash.eth.limo/) client library, designed to interface with Tornado Cash's smart contracts. It provides support for:
- Merkle tree syncing & storage
- Reorg recovery
- Deposit and withdrawal transaction generation

## Example

```rust,no_run
use alloy::providers::{Provider, ProviderBuilder};
use kohaku_kv_store::memory::MemoryStore;
use kohaku_tornadocash::{
    circuit::Circuit,
    indexer::rpc::RpcSyncer,
    provider::{pool::Pool, pool_provider::PoolProvider},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let provider = ProviderBuilder::new()
        .connect_http("http://localhost:8545".parse()?)
        .erased();

    let store = MemoryStore::new();
    let syncer = RpcSyncer::new(provider.clone());
    let circuit = Circuit::from_remote().await?;
    let pool = Pool::SEPOLIA_ETHER_01;

    let mut pool_provider = PoolProvider::new(
        pool,
        provider,
        store.into(),
        syncer.clone().into(),
        syncer.into(),
        circuit,
    );

    pool_provider.sync().await?;

    let (deposit_call, note) = pool_provider.deposit(&mut rand::rng());
    println!("Deposit call: {deposit_call:?}");
    println!("Deposit note: {note:?}");

    Ok(())
}
```

## Benchmarks

Benchmarks were run on a Ryzen 5 3600, 32GB RAM.

| Method | Target           | Time (ms) |
| ------ | ---------------- | --------- |
| prove  | native           | 1,442     |
| prove  | native +parallel | 485.72    |
