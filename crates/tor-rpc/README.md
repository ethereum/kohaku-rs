# kohaku-tor-rpc

Alloy JSON-RPC and plain HTTP over Tor, using [Arti](https://gitlab.torproject.org/tpo/core/arti).

An RPC provider sees the IP address of every request it serves. Sending requests through Tor hides that address, but requests that travel over the same circuit can still be linked to each other: queries about one address followed by queries about another look like one user. This crate exposes Arti's stream isolation so the caller decides which requests may share circuits.

## Shared and isolated clients

- [`TorRpc::connect`] bootstraps one Tor client. Clones of it share its circuits and its HTTP connection pool.
- [`TorRpc::isolated`] returns a new isolation group. Its streams never share circuits with the client it was created from or with any other isolated client. Clones of the returned value share that group.

Isolation is per group, not per request. Calling `isolated` once and reusing the value keeps every request on that group's circuits. Call it again for the next group that must not be linked to the previous one.

A typical split:

- Use the bootstrapped client for requests that are safe to link and say nothing about a particular user, such as `eth_blockNumber`, `eth_getLogs` over a contract's full event range, or a query to a PIR server.
- Take a new isolated client for each session about one user or address, and drop it when the session ends.

[`TorRpc::send`] sends one JSON-RPC packet as a single HTTP POST, so batch the calls that belong to one group into one packet.

## Example

```rust,no_run
# async fn example() -> anyhow::Result<()> {
use alloy::{primitives::address, providers::Provider};
use kohaku_tor_rpc::TorRpc;

let tor = TorRpc::connect().await?;
let url: url::Url = "https://ethereum-rpc.publicnode.com".parse()?;

// Linkable traffic that says nothing about the user: the shared client.
let head = tor.provider(url.clone()).get_block_number().await?;

// Calls about one address: a fresh isolation group for this session.
let session = tor.isolated();
let provider = session.provider(url);
let account = address!("0x0000000000000000000000000000000000000001");
let balance = provider.get_balance(account).await?;
let nonce = provider.get_transaction_count(account).await?;
# Ok(())
# }
```

Plain HTTP goes through the same circuits. [`TorRpc::get`], [`TorRpc::post`] and [`TorRpc::request`] return the body and fail on a non-2xx status. [`TorRpc::http`] returns the full response, whatever its status.

```rust,no_run
# async fn example(tor: kohaku_tor_rpc::TorRpc, query: Vec<u8>) -> anyhow::Result<()> {
let url: url::Url = "https://pir.example.com/query".parse()?;
let headers = [("content-type".to_owned(), "application/octet-stream".to_owned())];
let answer = tor.post(&url, &headers, &query).await?;
# Ok(())
# }
```

## Notes

- `connect` uses Arti's default configuration, including its default state and cache directories, and fails if the Tor network cannot be reached.
- Arti refuses local addresses (`localhost`, private ranges) by default. The error appears when a request is sent, not when a client is built.
- HTTP/1.1 only. `https` URLs use rustls with the Mozilla root certificates from `webpki-roots`.
- `connect` installs `ring` as the process-wide rustls `CryptoProvider` unless one is already installed.
- Native targets only. The crate is excluded from the `wasm32` build.

## Testing

The only test bootstraps Tor and calls a public RPC, so it is `#[ignore]`d. It prints the time of shared and isolated calls.

```sh
cargo test -p kohaku-tor-rpc -- --ignored --nocapture
```
