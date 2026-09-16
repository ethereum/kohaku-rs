# kohaku-pir-provider

Helios-style dual-endpoint Ethereum JSON-RPC provider. A small allowlist of
methods is answered via `pir-client` from inspire-gpu-serving (private lookup).
Everything else is forwarded to a normal Ethereum node.

```text
get_balance / get_transaction_count / matched eth_call
        → PIR server (pir-front / pir-server)
everything else (eth_getLogs, unmatched eth_call, …)
        → fallback JSON-RPC
```

Two URLs, always:

- `--pir-url` / `pir_url` — PIR serving front (`http://127.0.0.1:8080`)
- `--rpc-url` / `rpc_url` — ordinary Ethereum JSON-RPC

## Native (alloy)

```rust,ignore
use kohaku_pir_provider::{PirConnect, PirRouter};
use alloy::providers::ProviderBuilder;

let router = PirRouter::connect(config).await?; // requires `--features client`
let provider = ProviderBuilder::default()
    .connect_with(&PirConnect::new(std::sync::Arc::new(router)))
    .await?;
let wei = provider.get_balance(address).await?;
```

Live PIR (`PirRouter::connect`, `pir-rpc`, `connect_provider`) is behind the
`client` cargo feature: it links `pir-client` (C++ / OpenSSL) and needs the
inspire-gpu submodule.

```bash
cargo run -p kohaku-pir-provider --features client --bin pir-rpc -- \
  --pir-url http://127.0.0.1:8080 --rpc-url http://127.0.0.1:8545 --listen 127.0.0.1:8546
cast balance 0xabc... --rpc-url http://127.0.0.1:8546
```

Point `@kohaku-eth/provider/pir` at that listen address as `pirUrl`.

## `eth_call` routing

PIR cannot run the EVM. A matched static call means the client recognizes
`(to, selector)`, derives a PIR key from the dataset advertisement on
`/manifest`, looks the key up, and ABI-encodes the bytes.

Unknown `(to, selector)` stays on the fallback RPC. Do not add call routes
until the PIR server actually indexes that dataset.

## Privacy

A PIR miss is proven non-membership in **this** database (the chain follower
is incomplete). Allowlisted methods do **not** silently fall back — that would
leak the address to the RPC node. Missing accounts return `0x0`, matching
ordinary `eth_getBalance` for empty accounts.
