# kohaku-tornadocash

Rust [Tornadocash](https://tornadocash.eth.limo/) client library, designed to interface with Tornado Cash's smart contracts. It provides support for:
- Merkle tree syncing & storage
- Reorg recovery
- Multi-pool management
- Deposit and withdrawal transaction generation
- [Relayed](./src/relayer/) withdrawal transactions
- [Bundled](./src/userop_provider/) withdrawal transactions

## Example

### Depositing into a Tornado Cash pool

```rust,no_run
use alloy::providers::{Provider, ProviderBuilder};
use kohaku_kv_store::Store;
use kohaku_tornadocash::{
    indexer::rpc::RpcSyncer,
    pool::Pool,
    provider::TornadoProvider,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let provider = ProviderBuilder::new()
        .connect_http("http://localhost:8545".parse()?)
        .erased();

    let syncer = RpcSyncer::new(provider.clone());
    let mut tornado_provider = TornadoProvider::new(
        Store::create(),
        syncer.clone().into(),
        syncer.into(),
        provider.clone(),
    );

    tornado_provider.sync().await?;

    let deposit = tornado_provider.deposit(Pool::SEPOLIA_ETHER_01, &mut rand::rng());
    let note = deposit.note();
    Ok(())
}
```

### Withdrawing directly from a Tornado Cash pool

```rust,no_run
use alloy::providers::{Provider, ProviderBuilder};
use kohaku_kv_store::Store;
use kohaku_tornadocash::{
    indexer::rpc::RpcSyncer,
    pool::Pool,
    provider::TornadoProvider,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
#     let provider = ProviderBuilder::new()
#         .connect_http("http://localhost:8545".parse()?)
#         .erased();
# 
#     let syncer = RpcSyncer::new(provider.clone());
# 
#     let mut tornado_provider = TornadoProvider::new(
#         Store::create(),
#         syncer.clone().into(),
#         syncer.into(),
#         provider.clone(),
#     );
# 
#     tornado_provider.sync().await?;
# 
    let note = "tornado-eth-0.1-11155111-0xsecret".parse()?;
    let recipient = "0xrecipient".parse()?;
    let withdrawal = tornado_provider.withdraw(note, recipient).await?;
    
    Ok(())
}
```

### Withdrawing via a Tornado Cash relayer

```rust,no_run
use alloy::providers::{Provider, ProviderBuilder};
use alloy::primitives::U256;
use kohaku_kv_store::Store;
use kohaku_tornadocash::{
    indexer::rpc::RpcSyncer,
    pool::Pool,
    provider::TornadoProvider,
    relayer::Relayer,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
#     let provider = ProviderBuilder::new()
#         .connect_http("http://localhost:8545".parse()?)
#         .erased();
# 
#     let syncer = RpcSyncer::new(provider.clone());
#     let mut tornado_provider = TornadoProvider::new(
#         Store::create(),
#         syncer.clone().into(),
#         syncer.into(),
#         provider.clone(),
#     );
# 
#     tornado_provider.sync().await?;
# 
    let relayer_url = "https://mainnet.relayer.com";
    let relayer = Relayer::new(relayer_url);

    let note = "tornado-eth-0.1-11155111-0xsecret".parse()?;
    let recipient = "0xrecipient".parse()?;
    let receipt = tornado_provider.withdraw(note, recipient).await?.relay(&relayer, &mut rand::rng()).await?;
    let tx_hash = relayer.await_confirmation(&tornado_provider, &receipt).await?;

    Ok(())
}
```

### Withdrawing via a UserOperation

```rust,no_run
use alloy::providers::{Provider, ProviderBuilder};
use alloy::signers::local::PrivateKeySigner;
use kohaku_kv_store::Store;
use kohaku_tornadocash::{
    indexer::rpc::RpcSyncer,
    pool::Pool,
    provider::TornadoProvider,
    userop_provider::WithdrawalPaymasterExt,
};
use kohaku_userop_kit::{
    builder::UserOperationBuilder,
    bundler::{Bundler, pimlico::PimlicoBundler},
    smart_account::simple_7702_smart_account::{Call, Simple7702SmartAccount},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
#     let provider = ProviderBuilder::new()
#         .connect_http("http://localhost:8545".parse()?)
#         .erased();
# 
#     let syncer = RpcSyncer::new(provider.clone());
#     let tornado_provider = TornadoProvider::new(
#         Store::create(),
#         syncer.clone().into(),
#         syncer.into(),
#         provider.clone(),
#     );

    let owner = PrivateKeySigner::random();
    let note = "tornado-eth-0.1-11155111-0xsecret".parse()?;
    let smart_account = Simple7702SmartAccount::new(provider.clone(), owner.address(), 11155111);
    
    let bundler = PimlicoBundler::new("https://bundler.pimlico.com".parse()?);

    let builder = UserOperationBuilder::new_with_smart_account(&smart_account).await?;
    let builder = tornado_provider
        .withdraw(note, owner.address())
        .await?
        .sponsor(&bundler, builder, &mut rand::rng())
        .await?;

    let userop = builder.build().sign(&owner).await?;

    Ok(())
}
```

## Benchmarks

Benchmarks were run on a Ryzen 5 3600, 32GB RAM.

| Method | Target           | Time (ms) |
| ------ | ---------------- | --------- |
| prove  | native           | 1,442     |
| prove  | native +parallel | 485.72    |
