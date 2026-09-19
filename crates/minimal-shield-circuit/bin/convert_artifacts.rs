//! Convert MSP `spend_final.zkey` + `spend.wasm` into brotli-compressed ark-circom artifacts.

use std::{
    fs,
    io::{Cursor, Write},
    path::PathBuf,
};

use ark_circom::read_zkey;
use ark_serialize::CanonicalSerialize;
use kohaku_minimal_shield_circuit::matrices::SerializableNpIndex;

fn main() {
    let mut args = std::env::args().skip(1);
    let zkey = PathBuf::from(args.next().expect("usage: convert-msp-artifacts <zkey> <wasm> <out-dir>"));
    let wasm = PathBuf::from(args.next().expect("usage: convert-msp-artifacts <zkey> <wasm> <out-dir>"));
    let out = PathBuf::from(args.next().expect("usage: convert-msp-artifacts <zkey> <wasm> <out-dir>"));
    fs::create_dir_all(&out).unwrap();

    let zkey_bytes = fs::read(&zkey).expect("read zkey");
    let (pk, matrices) = read_zkey(&mut Cursor::new(zkey_bytes)).expect("parse zkey");
    let matrices: SerializableNpIndex<_> = matrices.into();

    let params = brotli::enc::BrotliEncoderParams::default();

    let mut pk_bytes = Vec::new();
    pk.serialize_uncompressed(&mut pk_bytes).unwrap();
    let mut pk_file = fs::File::create(out.join("proving_key.bin.br")).unwrap();
    brotli::BrotliCompress(&mut pk_bytes.as_slice(), &mut pk_file, &params).unwrap();
    pk_file.flush().unwrap();

    let mut mat_bytes = Vec::new();
    matrices.serialize_uncompressed(&mut mat_bytes).unwrap();
    let mut mat_file = fs::File::create(out.join("matrices.bin.br")).unwrap();
    brotli::BrotliCompress(&mut mat_bytes.as_slice(), &mut mat_file, &params).unwrap();
    mat_file.flush().unwrap();

    let wasm_bytes = fs::read(&wasm).expect("read wasm");
    let mut wasm_file = fs::File::create(out.join("wasm.br")).unwrap();
    brotli::BrotliCompress(&mut wasm_bytes.as_slice(), &mut wasm_file, &params).unwrap();
    wasm_file.flush().unwrap();

    println!("wrote artifacts to {}", out.display());
}
