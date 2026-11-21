# tornadocash-syncer

Syncers for retrieving Tornado Cash notes and Merkle tree data.

## Examples

### RPC

```rust,no_run
# use alloy::providers::DynProvider;
# use kohaku_tornadocash::{
#     merkle_tree::{MerkleTree, MerkleTreeExt},
#     Pool,
# };
# use kohaku_tornadocash_syncer::{Syncer, rpc::RpcSyncer};
#
# async fn example(provider: DynProvider) -> Result<(), Box<dyn std::error::Error>> {
let syncer = RpcSyncer::new(provider);
let snapshot = syncer.sync(&Pool::SEPOLIA_ETHER_01, ..).await?;

let mut tree = MerkleTree::new();
tree.splice_events(&snapshot)?;

# Ok(())
# }
```

### Remote

```rust,no_run
# use kohaku_tornadocash::{
#     merkle_tree::{MerkleTree, MerkleTreeExt},
#     Pool,
# };
# use kohaku_tornadocash_syncer::{Syncer, remote::RemoteSyncer};
#
# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let syncer = RemoteSyncer::new("https://example.com/tornadocash-sync");
let snapshot = syncer.sync(&Pool::SEPOLIA_ETHER_01, ..).await?;

let mut tree = MerkleTree::new();
tree.splice_events(&snapshot)?;

# Ok(())
# }
```

### Saga

Requires the `saga-sync` feature.

```rust,no_run
# use kohaku_tornadocash::{
#     merkle_tree::{MerkleTree, MerkleTreeExt},
#     Pool,
# };
# use kohaku_tornadocash_syncer::Syncer;
#
# #[cfg(feature = "saga-sync")]
# async fn example() -> Result<(), Box<dyn std::error::Error>> {
#     use kohaku_tornadocash_syncer::saga::SagaSyncer;
let syncer = SagaSyncer::new("https://saga.fatsolutions.xyz/sepolia");
let snapshot = syncer.sync(&Pool::SEPOLIA_ETHER_01, ..).await?;

let mut tree = MerkleTree::new();
tree.splice_events(&snapshot)?;

# Ok(())
# }
```
