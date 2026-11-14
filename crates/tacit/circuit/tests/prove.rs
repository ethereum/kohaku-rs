//! Proves the reference witnesses with the ceremony artifacts.
//!
//! The artifacts are not part of the crate (about 33 MB). Point `TACIT_ARTIFACTS_DIR` at a
//! directory holding `transact.wasm` and `transact_final.zkey` and run with `--release --
//! --ignored`.

use std::{path::PathBuf, str::FromStr, time::Instant};

use kohaku_tacit_circuit::{CircuitError, CircuitInputs, Prover, TREE_DEPTH};
use ruint::aliases::U256;
use serde_json::Value;

fn vectors() -> Value {
    serde_json::from_str(include_str!("fixtures/reference-vectors.json")).unwrap()
}

fn num(v: &Value) -> U256 {
    U256::from_str(v.as_str().expect("decimal string")).unwrap()
}

fn arr<const N: usize>(v: &Value) -> [U256; N] {
    let items = v.as_array().unwrap();
    assert_eq!(items.len(), N);
    std::array::from_fn(|i| num(&items[i]))
}

fn pairs<const N: usize>(v: &Value) -> [[U256; N]; 2] {
    let items = v.as_array().unwrap();
    [arr(&items[0]), arr(&items[1])]
}

fn inputs(v: &Value) -> CircuitInputs {
    CircuitInputs {
        root: num(&v["root"]),
        old_root: num(&v["oldRoot"]),
        new_root: num(&v["newRoot"]),
        start_index: num(&v["startIndex"]),
        public_amount: num(&v["publicAmount"]),
        ext_data_hash: num(&v["extDataHash"]),
        asset: num(&v["asset"]),
        nf: arr(&v["nf"]),
        out_leaf: arr(&v["outLeaf"]),
        in_v: arr(&v["inV"]),
        in_rho: arr(&v["inRho"]),
        in_nk: arr(&v["inNk"]),
        in_ak: pairs(&v["inAk"]),
        in_index: arr(&v["inIndex"]),
        in_path: pairs::<TREE_DEPTH>(&v["inPath"]),
        sig_r8: pairs(&v["sigR8"]),
        sig_s: arr(&v["sigS"]),
        out_v: arr(&v["outV"]),
        out_npk: arr(&v["outNpk"]),
        out_rho: arr(&v["outRho"]),
        ins_path: arr(&v["insPath"]),
    }
}

fn prover() -> Prover {
    let dir = PathBuf::from(
        std::env::var("TACIT_ARTIFACTS_DIR").expect("set TACIT_ARTIFACTS_DIR to the artifacts"),
    );
    let wasm = std::fs::read(dir.join("transact.wasm")).unwrap();
    let zkey = std::fs::read(dir.join("transact_final.zkey")).unwrap();
    let start = Instant::now();
    let prover = Prover::new(&wasm, &zkey).unwrap();
    eprintln!("loaded in {:?}", start.elapsed());
    prover
}

#[test]
#[ignore = "needs the ceremony artifacts (TACIT_ARTIFACTS_DIR); run with --release"]
fn proves_the_reference_witnesses() {
    let vectors = vectors();
    let prover = prover();
    let mut rng = rand::rng();

    for name in ["witnessDeposit", "witnessSpend"] {
        let inputs = inputs(&vectors[name]["input"]);
        let public: [U256; 11] = arr(&vectors[name]["publicSignals"]);
        assert_eq!(inputs.public_signals(), public);

        let start = Instant::now();
        let proof = prover.prove(&inputs, &mut rng).unwrap();
        eprintln!("{name}: proved in {:?}", start.elapsed());
        assert!(prover.verify(&proof, &public));

        let mut other = public;
        other[4] += U256::from(1);
        assert!(!prover.verify(&proof, &other));
    }
}

#[test]
#[ignore = "needs the ceremony artifacts (TACIT_ARTIFACTS_DIR); run with --release"]
fn refuses_inputs_the_circuit_does_not_accept() {
    let vectors = vectors();
    let prover = prover();
    let mut rng = rand::rng();

    // An output worth more than the inputs bring in.
    let mut inputs = inputs(&vectors["witnessSpend"]["input"]);
    inputs.out_v[0] += U256::from(1);
    assert!(matches!(
        prover.prove(&inputs, &mut rng),
        Err(CircuitError::Witness(_))
    ));

    // Public signals that are not the ones the witness computes.
    let mut inputs = self::inputs(&vectors["witnessDeposit"]["input"]);
    inputs.new_root += U256::from(1);
    assert!(prover.prove(&inputs, &mut rng).is_err());
}

#[test]
fn refuses_artifacts_that_differ_from_the_pins() {
    assert!(matches!(
        Prover::new(b"not the program", b"not the key"),
        Err(CircuitError::PinMismatch("transact.wasm"))
    ));
}
