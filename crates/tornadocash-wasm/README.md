# kohaku-tornadocash-wasm

WebAssembly bindings for the Rust `kohaku-tornadocash` core. The crate builds
an `rlib` and a `cdylib`; wasm-bindgen generates the JavaScript and TypeScript
files. SDK and Worker integration are separate work.

## Note API

`src/note.rs` defines `Note`, which owns a core Rust note, and `NoteData`, the
plain object used to exchange its fields with JavaScript. `src/lib.rs` reexports
both types. The generated API includes:

```ts
export class Note {
  constructor(data: NoteData);
  static random(symbol: string, amount: string, chainId: bigint): Note;
  static parse(text: string): Note;
  toObject(): NoteData;
  toString(): string;
  preimage(): Uint8Array;
  commitment(): string;
  nullifierHash(): string;
  free(): void;
}

export interface NoteData {
  symbol: string;
  amount: string;
  chainId: bigint;
  nullifier: Uint8Array;
  secret: Uint8Array;
}
```

All operations run synchronously and delegate to the stored core note. Multiple
notes share the loaded WASM instance. Release each note after use:

```ts
const note = Note.parse(originalText);
try {
  const commitment = note.commitment();
  const fields = note.toObject();
} finally {
  note.free();
}
```

`free()` releases the Rust object; methods must not be called afterward.
Releasing one note leaves other notes usable. wasm-bindgen also provides
`[Symbol.dispose]()` and automatic cleanup through `FinalizationRegistry`
where available, but explicit cleanup makes the lifetime predictable.

### Data and conversions

- The constructor copies its input. `toObject()` returns a plain object with
  independent copies of the fields, including the original secret bytes.
- `chainId` must be a `bigint` in the `u64` range. `nullifier` and `secret` must
  each be a `Uint8Array` of exactly 31 bytes. Plain arrays and `number` chain IDs
  are rejected at runtime. Conversion and parsing failures throw JS `Error`s.
- Byte validation uses the current JS context's `Uint8Array` constructor. Arrays
  created in another context, such as an iframe or Node's `vm`, are not supported
  directly. Copy them with the receiving context's `Uint8Array.from(bytes)` first.
- Symbol and amount remain strings with the core's existing behavior. No extra
  metadata validation is added; arbitrary strings may produce text that cannot
  be parsed back.
- `toString()` uses the core's legacy format:
  `tornado-{symbol}-{amount}-{chainId}-0x{preimage}`. Parsing accepts an optional
  `0x` prefix and does not trim whitespace. Formatting emits lowercase hex and
  canonical decimal chain IDs while preserving the amount text.
- `preimage()` returns a copy of 62 bytes: nullifier first, then secret.
  Snapshots and preimages remain usable after freeing the note; modifying them
  does not change the stored note.
- Both hashes are `0x` followed by 64 lowercase hex digits (most significant byte
  first). Commitment depends on nullifier and secret; nullifier hash depends only
  on nullifier. Neither uses metadata or checks deposits or spent status.

wasm-bindgen exports the class, methods and primitive return types. Serde handles
structured conversion, and tsify generates the `NoteData` interface. The adapter
uses `Ts<NoteData>` with explicit, fallible `to_rust()` / `into_ts()` conversions.

Serde annotations map `chain_id` to `chainId`; tsify represents `u64` as `bigint`.
`serde_bytes::serialize` produces typed byte arrays, declared as `Uint8Array` via
tsify. Custom deserializers preserve the incoming JS values, check their type
and range/length, then copy them into Rust. Requiring these specific JS types is
an API choice; Serde's default conversion also accepts some other representations.

### Random generation

```ts
const note = Note.random('eth', '0.1', 1n);
try {
  const text = note.toString();
} finally {
  note.free();
}
```

The adapter seeds `StdRng` with `StdRng::try_from_rng(&mut SysRng)?` and passes it
to `CoreNote::random`. The core generates the nullifier and secret; the caller
supplies only metadata. The generator is discarded after each call.

On WASM, `getrandom`'s `wasm_js` feature obtains the seed through
`globalThis.crypto.getRandomValues()`. The host must provide Web Crypto; an absent
or failing source throws a JS `Error`, with no insecure fallback. `random()`
checks the chain ID with `u64::try_from` to reject invalid types and out-of-range
values rather than truncate them.

## Build and test

Run commands from the repository root. Use the `wasm32-unknown-unknown` Rust
target and a wasm-bindgen CLI matching the pinned dependency:

```sh
cargo install wasm-bindgen-cli --version 0.2.108 --locked

cargo build --locked --manifest-path crates/Cargo.toml \
  -p kohaku-tornadocash-wasm --target wasm32-unknown-unknown --target-dir crates/target
```

### Node and TypeScript

```sh
wasm-bindgen crates/target/wasm32-unknown-unknown/debug/kohaku_tornadocash_wasm.wasm \
  --target nodejs --out-dir crates/target/tornadocash-wasm-node

TORNADOCASH_WASM_MODULE=crates/target/tornadocash-wasm-node/kohaku_tornadocash_wasm.js \
  node --test crates/tornadocash-wasm/tests/*.cjs

npm exec --yes --package=typescript@7.0.2 -- tsc --noEmit --strict \
  --target ES2020 --lib ES2020,ESNext.Disposable --module Node16 crates/tornadocash-wasm/tests/*.types.ts
```

The `.cjs` tests exercise the generated `CommonJS` bindings: byte order, chain ID
boundaries, invalid inputs, formatting, independent snapshots, object lifetimes,
hashes, and Web Crypto failure/recovery. The `.types.ts` files are compile-only
consumers that check accepted and rejected types against the generated declarations.
`ESNext.Disposable` supplies the types for `[Symbol.dispose]()`.

These tests cover the JS-specific conversions in a WASM runtime. The core's Rust
tests cover its own implementation. Random generation checks are integration
tests, not a statistical assessment of the generator.

### Browser

Generate the web bindings from the same build, then install the test tooling:

```sh
wasm-bindgen crates/target/wasm32-unknown-unknown/debug/kohaku_tornadocash_wasm.wasm \
  --target web --out-dir crates/target/tornadocash-wasm-web

npm ci --prefix crates/tornadocash-wasm/tests/browser
(cd crates/tornadocash-wasm/tests/browser && npx --no-install playwright install chromium)
npm test --prefix crates/tornadocash-wasm/tests/browser
```

Playwright opens headless Chromium against a temporary localhost server. Tests
load the generated ES module and check conversions, reference hashes, invalid
inputs, and Web Crypto failure/recovery. Build outputs stay in `crates/target`.

To use an installed Chromium, skip the browser download and run:

```sh
PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH=/usr/bin/chromium \
  npm test --prefix crates/tornadocash-wasm/tests/browser
```

### Reference hash vectors

`tests/fixtures/note-vectors.json` contains synthetic inputs and expected hashes
from [circomlibjs](https://github.com/iden3/circomlibjs/blob/v0.1.7/src/pedersen_hash.js).
The reference hashes the 62-byte preimage or 31-byte nullifier, unpacks the
`BabyJubJub` point, and encodes its x-coordinate as a 32-byte hex string. Vectors
cover repeated, ordered and zero bytes, including leading-zero hash padding.

To reproduce the vectors for review:

```sh
npm install --prefix crates/target/note-vector-reference --ignore-scripts --no-audit --no-fund circomlibjs@0.1.7
node crates/tornadocash-wasm/tests/fixtures/generate-note-vectors.cjs \
  crates/target/note-vector-reference/node_modules/circomlibjs \
  > crates/target/note-vectors.json
diff -u crates/tornadocash-wasm/tests/fixtures/note-vectors.json crates/target/note-vectors.json
```

Tests read the checked-in vectors; they never regenerate expected hashes from
the implementation under test. This covers note hashing, not full TS SDK parity.

## CI

The `WASM` workflow runs on PRs targeting `master`, pushes to `master`, and manual
dispatch. It builds the crate, runs strict Clippy for this crate (`--no-deps --
-D warnings`), generates Node and web bindings, and runs the Node, TypeScript and
Chromium checks. Any failed step fails the job.

CI uses Rust 1.98.1, Node 26.8.1, wasm-bindgen CLI 0.2.108, TypeScript 7.0.2,
and Playwright 1.63.0 with its matching Chromium. Keep the wasm-bindgen dependency
and CLI versions aligned. To reproduce the lint check locally:

```sh
cargo clippy --locked --manifest-path crates/Cargo.toml \
  -p kohaku-tornadocash-wasm --target wasm32-unknown-unknown \
  --target-dir crates/target --no-deps -- -D warnings
```
