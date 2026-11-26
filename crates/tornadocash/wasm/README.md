# kohaku-tornadocash-wasm

Typescript bindings for `kohaku-tornadocash` and its sibling crates.

`kohaku-tornadocash-wasm`'s WASM bindings are a lightweight wrapper around the Rust library. For documentation on the underlying Rust library, see the [`core`], [`circuit`], [`syncer`], [`relayer`], [`paymaster`], and [`keychain`] crates.

[`core`]: https://docs.rs/kohaku-tornadocash/latest/
[`circuit`]: https://docs.rs/kohaku-tornadocash-circuit/latest/
[`syncer`]: https://docs.rs/kohaku-tornadocash-syncer/latest/
[`relayer`]: https://docs.rs/kohaku-tornadocash-relayer/latest/
[`paymaster`]: https://docs.rs/kohaku-tornadocash-paymaster/latest/
[`keychain`]: https://docs.rs/kohaku-tornadocash-keychain/latest/
[`wasm`]: https://docs.rs/kohaku-tornadocash-wasm/latest/

## Build

From the repository root, enter the committed CI shell with `nix develop .#ci`.
It supplies Rust 1.98.0, the WASM target, wasm-bindgen CLI 0.2.108 (matching the
workspace library), and Node 26. Keep `crates/Cargo.lock` unchanged when building.

```sh
cargo build --release --locked --manifest-path crates/Cargo.toml \
  -p kohaku-tornadocash-wasm --target wasm32-unknown-unknown --target-dir crates/target
wasm-bindgen crates/target/wasm32-unknown-unknown/release/kohaku_tornadocash_wasm.wasm \
  --target nodejs --out-dir crates/target/tornadocash-wasm-release-node
wasm-bindgen crates/target/wasm32-unknown-unknown/release/kohaku_tornadocash_wasm.wasm \
  --target web --out-dir crates/target/tornadocash-wasm-release-web
```

The integration tests use these release outputs directly, rather than a published
npm package. If using another output location, set
`KOHAKU_TORNADO_NODE_BINDINGS` to the generated Node `.js` file and/or
`KOHAKU_TORNADO_WEB_BINDINGS` to the generated web directory. Proving requires
release-optimized WASM.

## Integration tests

The existing native-conversion and Rust-authored WASM tests run with:

```sh
cargo test --locked --manifest-path crates/Cargo.toml \
  -p kohaku-tornadocash-wasm --lib --target-dir crates/target
cargo test --release --locked --manifest-path crates/Cargo.toml \
  -p kohaku-tornadocash-wasm --lib --target wasm32-unknown-unknown --target-dir crates/target
```

After generating bindings, run the JavaScript tests from the repository root:

```sh
node --test crates/tornadocash/wasm/tests/js/*.test.cjs
```

These exercise generated constructors, deposit/approval calldata, Merkle
operations, runtime artifact loading and real withdrawal proving, same-version
JSON round trips, Node worker proving, event decoding, provider queries, remote
cache synchronization, and offline relayer quotes. HTTP/RPC/cache fixtures run on
loopback; the tests require no public service, account or live funds. Proving
artifacts come from the repository's existing compressed circuit fixture.

The circuit benchmark's `signals.json` supplies an independent nullifier-hash
vector; ABI selectors and event topics were derived from the Solidity signatures
with `cast`. Withdrawal proof bytes are randomized. Tests check generated calls,
round trips and inclusion proofs, not independent cryptographic verification of
the generated Groth16 proof.

The Snapshot event round-trip regression executes with a TODO annotation on the
current base because it depends on [#159](https://github.com/ethereum/kohaku-rs/pull/159).
It is an existing upstream failure, not a passing case. Remove the TODO annotation
and rerun after that fix merges. The pending Relayer HTTP API in #162 is not part
of this revision's exported surface.

### Browser tests

From the repository root, in the same Node environment:

```sh
cd crates/tornadocash/wasm/tests/js
npm ci --ignore-scripts --no-audit --no-fund
npx playwright install --with-deps chromium
npm run test:browser
```

The development-only driver runs generated web bindings in headless Chromium
using a temporary test profile and local fixture server. To use an already
installed compatible Chromium/Chrome, set `KOHAKU_CHROMIUM_EXECUTABLE` to its
executable and omit the browser installation command. Node and web bindings must
come from the same source revision. Browser worker coverage checks existing JSON
transport; the separate Node worker case performs real proving. Optional parallel
WASM builds and browser-worker proving optimizations are outside this suite.
