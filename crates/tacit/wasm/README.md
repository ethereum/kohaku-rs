# kohaku-tacit-wasm

JavaScript bindings for `kohaku-tacit`, `kohaku-tacit-circuit` and `kohaku-tacit-relayer`: a thin wrapper around the Rust library. Proving runs inside the module, so no snarkjs is needed.

Amounts are `bigint` wei. Addresses are strings: `0x…` for Ethereum accounts, `bp1…` for pool addresses.

```js
const { TacitAccount, TacitPool, TacitWallet, TacitProver, TacitPayer } = require(
  './crates/target/tacit-wasm-node/kohaku_tacit_wasm.js',
);

const prover = new TacitProver(wasmBytes, zkeyBytes); // transact.wasm, transact_final.zkey
const account = TacitAccount.fromMnemonic(phrase, 0);
const wallet = new TacitWallet(account, TacitPool.ethereum());
await wallet.sync(rpcUrl);

const tx = wallet.unshield(recipient, 10n ** 16n, TacitPayer.own(), wallet.tree());
const { to, data, value } = tx.prove(prover).call(); // send from the user's account
```

Through a relayer:

```js
const relayer = new TacitRelayer(TacitPool.ethereum());
const payer = await relayer.quote();
const tree = await relayer.provingTree(wallet, rpcUrl);
const hash = await relayer.relay(wallet.transfer(to, amount, payer, tree).prove(prover));
```

`wallet.save()` returns the confirmed state as JSON, and `TacitWallet.restore(account, pool, json)` resumes from it.

## Build

```sh
cargo build -p kohaku-tacit-wasm --target wasm32-unknown-unknown --release
wasm-bindgen target/wasm32-unknown-unknown/release/kohaku_tacit_wasm.wasm \
  --target nodejs --out-dir target/tacit-wasm-node
```
