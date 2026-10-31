# kohaku-railgun-wasm

JavaScript/TypeScript bindings for the portable parts of `kohaku-railgun`.
Like `kohaku-tornadocash-wasm`, this crate keeps `wasm-bindgen` and `tsify`
outside the Rust core. It currently covers Railgun addresses, known chains,
and shield transaction preparation.

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

This crate does not own sync state or make PPOI or paymaster requests. Those
extensions are separate opt-ins in Rust. The WASM surface currently prepares
shields; it does not expose sync, spend proving, or PPOI.

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
