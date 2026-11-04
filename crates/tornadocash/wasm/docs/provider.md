# Pool queries

`TornadoProvider` delegates read-only pool queries to the canonical
[`TornadoProviderExt`](../../core/src/provider.rs) through an HTTP(S) RPC endpoint.
Each call captures owned inputs and a provider handle before returning its Promise.

Using the [generated Node bindings](../README.md#build), with a caller-supplied
`rpcUrl`, `pool`, nullifier hashes and root:

```js
const { TornadoProvider } = require(
  './crates/target/tornadocash-wasm-node/kohaku_tornadocash_wasm.js',
);

const provider = new TornadoProvider(rpcUrl);
try {
  const spent = await provider.isSpent(pool, nullifierHash);
  const statuses = await provider.isSpentArray(pool, nullifierHashes);
  const known = await provider.isKnownRoot(pool, root);
} finally {
  provider.free();
}
```
