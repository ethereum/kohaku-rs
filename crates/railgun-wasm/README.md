# kohaku-railgun-wasm

JavaScript/TypeScript bindings for Railgun. Like `kohaku-tornadocash-wasm`,
this crate keeps `wasm-bindgen` outside the Rust core. The Rust crates own
sync, proof generation, PPOI recovery, and paymaster `UserOperation` preparation.

## API

- `RailgunAddress.parse(text)` validates a Bech32m address.
- `RailgunAddress.fromPrivateKeys(spendingKey, viewingKey, chainId)` derives an
  address from two 32-byte secrets. The inputs must be protected as wallet keys.
- `RailgunAddress.fromPublicKeys(masterKey, viewingPublicKey, chainId)` accepts
  32-byte public keys; `allChains()` changes the advisory chain hint.
- `Chain.mainnet()`, `Chain.sepolia()`, and `Chain.fromChainId(chainId)` select
  the core's known deployments.
- `new Shield(chain)` collects `addErc20(recipient, token, amount)` and
  `addNative(recipient, amount)` entries. `build()` returns plain `{to, data,
  value}` transaction objects. It uses fresh cryptographic randomness for
  each shield and consumes the builder.
- `PoiSession.open(chain, rpcUrl, spendingKey, viewingKey, checkpointJson?,
  artifactBaseUrl?)` restores a wallet or starts a full resync. The RPC chain
  ID is checked against `chain`. The optional artifact base URL can point to a
  caller-controlled circuit mirror.
- `syncToWithoutPoiSubmission(block)` and `syncWithoutPoiSubmission()` sync
  UTXOs, TXIDs, and PPOI lists, then queue missing historical proofs. A fresh
  checkpoint replays chain history. Use bounded sync
  windows for resumable first syncs.
- `processPendingPoiJson()` generates and submits queued proofs. Its report
  distinguishes submitted proofs, TXIDs not yet indexed, inputs awaiting POI,
  and failures. Submission is **not** PPOI acceptance: sync the lists again
  before treating a note as valid. Failed work stays in the checkpoint.
- `balancesJson()`, `notesJson()`, `poiPendingJson()`, `poiListKeysJson()`,
  `poiRecoveredValidJson()`, and `poiStatusPerListJson(commitment)` expose
  local wallet and PPOI state. JSON amounts are decimal strings.
- `prepareTransferUserOpJson(...)` and `prepareUnshieldUserOpJson(...)` select
  PPOI-valid notes, prove the spend, estimate paymaster gas with a dummy proof,
  then return `{signed, userOpHash}`. Unshield takes a desired recipient amount
  and adds the Railgun treasury fee. Native unshield prepends WETH withdrawal;
  ordered tail calls follow it. Native unwrap and tail calls require the
  recipient to be the account that signs the `UserOperation`.

Hex inputs must have a `0x` prefix. Chain IDs and amounts are JavaScript
`bigint`s; values outside their Rust widths throw. The returned transaction
objects carry `value` as a `bigint` and `to` and `data` as hex strings. The
caller supplies transaction sender, gas, nonce, ERC20 approvals, and broadcast.

Example after generating the Node bindings:

```js
const { Chain, RailgunAddress, Shield } = require(
  './crates/target/railgun-wasm-node/kohaku_railgun_wasm.js',
);

const chain = Chain.mainnet();
const recipient = RailgunAddress.parse(recipientAddress); // Supply a real address.
const shield = new Shield(chain);
shield.addNative(recipient, 1000000000000000n);
const transactions = shield.build();
// Submit each transaction through your wallet/provider.
recipient.free();
chain.free();
```

`checkpointJson()` contains private wallet data. Encrypt it and save it after
every successful sync, proof-processing pass, and `UserOperation` preparation.
Save before broadcasting a prepared `UserOperation` so a crash cannot lose its
pending PPOI proof. A JS wrapper must handle encryption, checkpoints, progress
UX, Tor transport, transaction submission, and retries. Old combined SDK
checkpoints are not accepted; start from a fresh checkpoint to migrate them.
The PPOI node can still reject or delay a submitted proof, so the caller must
observe list inclusion before showing `Valid` or spending the output.

The `UserOperation` methods prepare and sign only. The caller sends `signed` to
the bundler, waits for a receipt, and syncs again. The bundler must support
state overrides for `eth_estimateUserOperationGas`. Tail calls are JSON objects
`{target, value, data}`; `value` is a decimal `uint256` string and `data` is
`0x`-prefixed hex. The account private key and Railgun keys are secrets; do not
log them or the checkpoint.

## Build

Run from the repository root. The wasm-bindgen CLI must match the Rust version.

```sh
rustup target add wasm32-unknown-unknown
cargo build --locked --manifest-path crates/Cargo.toml \
  -p kohaku-railgun-wasm --target wasm32-unknown-unknown --target-dir crates/target
wasm-bindgen crates/target/wasm32-unknown-unknown/debug/kohaku_railgun_wasm.wasm \
  --target nodejs --out-dir crates/target/railgun-wasm-node
wasm-bindgen crates/target/wasm32-unknown-unknown/debug/kohaku_railgun_wasm.wasm \
  --target web --out-dir crates/target/railgun-wasm-web
```

Both targets generate TypeScript declarations. For browser bindings, call the
default `init()` export before using the classes.

Run `node crates/railgun-wasm/tests/node-smoke.cjs` after generating Node
bindings to check session creation, checkpoint round-trip, and the PPOI report
with a local mock RPC.
