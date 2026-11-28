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

## JavaScript benchmarks

The Vitest benchmarks exercise release bindings in Node and headless Chromium.
From the repository root, enter `nix develop .#ci` and build the bindings:

```sh
cargo build --release --locked --manifest-path crates/Cargo.toml \
  -p kohaku-tornadocash-wasm --target wasm32-unknown-unknown --target-dir crates/target
wasm-bindgen crates/target/wasm32-unknown-unknown/release/kohaku_tornadocash_wasm.wasm \
  --target nodejs --out-dir crates/target/tornadocash-wasm-node
wasm-bindgen crates/target/wasm32-unknown-unknown/release/kohaku_tornadocash_wasm.wasm \
  --target web --out-dir crates/target/tornadocash-wasm-web
cd crates/tornadocash/wasm/benches/js
npm ci
npx playwright install --with-deps chromium
npm run bench
```

Set `KOHAKU_CHROMIUM_EXECUTABLE` to use an existing Chromium executable instead
of downloading one. Run `npm run bench -- --project=node` or
`npm run bench -- --project=chromium` for a single runtime.

Each workload runs five times with no warmup. CI sets `KOHAKU_BENCH_SMOKE=1` for
one repetition; it checks that benchmarks execute, without performance thresholds.
These small samples are performance indicators, not statistically stable claims.
Vitest prints results and writes raw samples under `benches/js/results/`, separated
by runtime. Record the commit, hardware, OS, Node/browser versions and build profile
when comparing runs; compare each runtime against its own baseline.

| Benchmark | Timed work | Excluded work |
| --- | --- | --- |
| Cold start | Fresh generated JS module and WASM initialization; Node loads the local module, Chromium fetches locally served JS/WASM | Process/browser startup and artifact loading |
| Artifact load | `CircuitArtifacts.fromBytes` decoding/deserialization | Reading/fetching bytes and Brotli decompression |
| Proving | `Withdrawal.prove` | Artifact loading, note/proof construction and output consumption |
| Merkle insertion | `MerkleTree.splice` with 1 or 128 leaves into a 1,024-leaf tree | Initial tree construction and root consumption |

The cold-start measurement creates a new binding/WASM instance each repetition;
it does not clear OS, HTTP or engine code caches. Artifact bytes come from the
repository's bundled circuit fixture and are decompressed before benchmarking.
Setup, result consumption and WASM object cleanup run outside timed operations.
No live RPC, credentials or chain access is required.
