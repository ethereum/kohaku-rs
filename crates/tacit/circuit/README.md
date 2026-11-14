# kohaku-tacit-circuit

The Tacit pool's `transact` circuit for Kohaku: witness generation and Groth16 proving from the pool's ceremony artifacts.

The circuit is compiled with circom 2, so its witness program is WebAssembly. This crate runs it in [`wasmi`](https://crates.io/crates/wasmi), a pure-Rust interpreter that also builds for `wasm32-unknown-unknown`, and proves with arkworks from the snarkjs proving key.

The artifacts are not embedded: the proving key is about 28.6 MB, above what a crate may carry. Load `transact.wasm` and `transact_final.zkey` from any mirror (see `artifacts::WASM_URLS` and `artifacts::ZKEY_URLS`). [`Prover::new`] refuses any copy whose SHA-256 differs from the pin. It also refuses a proving key whose verification key differs from the one the pool's verifier uses. Every proof is checked against that key before it is returned.

## Example

```rust,no_run
use kohaku_tacit_circuit::{CircuitInputs, Prover};

# fn example(wasm: &[u8], zkey: &[u8], inputs: &CircuitInputs) -> Result<(), Box<dyn std::error::Error>> {
let prover = Prover::new(wasm, zkey)?;
let proof = prover.prove(inputs, &mut rand::rng())?;
let (pa, pb, pc) = (proof.pa(), proof.pb(), proof.pc());
# Ok(())
# }
```

## Features

- `parallel`: parallel multi-scalar multiplications and FFTs with `rayon`.
