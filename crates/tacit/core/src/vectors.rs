//! Byte-for-byte agreement with the reference client.
//!
//! The fixture was produced by the reference implementation (see `source` in the file). Matching it
//! is what lets this wallet and the other Tacit apps read each other's notes.

use std::{convert::Infallible, str::FromStr};

use alloy::primitives::{Address, Bytes};
use rand_core::{TryCryptoRng, TryRng};
use ruint::aliases::U256;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{
    address::{Account, TacitAddress, secp_scalar},
    field::{Fr, Scalar, fr, scalar_word, word},
    keys::{leaf_of, nullifier_of, owned_keys, sign},
    memo::{open, seal},
    statement::{ExtData, OutputNote, SpendInput, asset_fr, build},
    tree::{IncTree, IncTreeState},
};

fn vectors() -> Value {
    serde_json::from_str(include_str!(
        "../../circuit/tests/fixtures/reference-vectors.json"
    ))
    .unwrap()
}

fn num(v: &Value) -> U256 {
    match v {
        Value::String(s) => U256::from_str(s).unwrap(),
        Value::Number(n) => U256::from(n.as_u64().unwrap()),
        _ => panic!("not a number: {v}"),
    }
}

fn f(v: &Value) -> Fr {
    fr(num(v)).unwrap()
}

fn bytes(v: &Value) -> Vec<u8> {
    hex::decode(v.as_str().unwrap().trim_start_matches("0x")).unwrap()
}

fn identity(v: &Value) -> Account {
    Account::from_identity(&bytes(v).try_into().unwrap()).unwrap()
}

/// The reference fixture's filler randomness: `sha256("xcheck-0") ‖ sha256("xcheck-1") ‖ …`, each
/// request starting a fresh block.
struct XcheckRng(u32);

impl TryRng for XcheckRng {
    type Error = Infallible;

    fn try_next_u32(&mut self) -> Result<u32, Infallible> {
        unimplemented!()
    }

    fn try_next_u64(&mut self) -> Result<u64, Infallible> {
        unimplemented!()
    }

    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Infallible> {
        for chunk in dst.chunks_mut(32) {
            let block = Sha256::digest(format!("xcheck-{}", self.0));
            self.0 += 1;
            chunk.copy_from_slice(&block[..chunk.len()]);
        }
        Ok(())
    }
}

impl TryCryptoRng for XcheckRng {}

#[test]
fn derives_the_reference_wallets() {
    let v = vectors();
    let one = identity(&v["identity1"]["key"]);
    assert_eq!(
        one.address().to_string(),
        v["identity1"]["address"].as_str().unwrap()
    );
    assert_eq!(scalar_word(one.keys.a), num(&v["identity1"]["a"]));
    assert_eq!(scalar_word(one.keys.n), num(&v["identity1"]["n"]));
    let two = identity(&v["identity2"]["key"]);
    assert_eq!(
        two.address().to_string(),
        v["identity2"]["address"].as_str().unwrap()
    );
}

#[test]
fn round_trips_addresses_and_refuses_bad_ones() {
    let v = vectors();
    let one = identity(&v["identity1"]["key"]);
    let text = one.address().to_string();
    assert_eq!(TacitAddress::from_str(&text).unwrap(), *one.address());
    assert_eq!(
        TacitAddress::from_str(&text.to_uppercase()).unwrap(),
        *one.address()
    );
    assert!(TacitAddress::from_str(&format!("t{text}")).is_err());

    // The identity point in place of A.
    let mut raw = one.address().to_bytes();
    raw[33..65].copy_from_slice(&crate::babyjubjub::IDENTITY.pack());
    assert!(TacitAddress::from_bytes(&raw).is_err());
}

#[test]
fn derives_note_keys_leaves_and_nullifiers() {
    let v = vectors();
    let one = identity(&v["identity1"]["key"]);
    let secret: [u8; 33] = bytes(&v["secret"]).try_into().unwrap();
    let owned = owned_keys(&one.keys, &secret).unwrap();
    assert_eq!(scalar_word(owned.sk), num(&v["owned"]["sk"]));
    assert_eq!(scalar_word(owned.nk), num(&v["owned"]["nk"]));
    assert_eq!(word(owned.npk), num(&v["owned"]["npk"]));
    assert_eq!(word(owned.rho), num(&v["owned"]["rho"]));

    let pool: Address = v["pool"].as_str().unwrap().parse().unwrap();
    let asset = asset_fr(1, pool, Address::ZERO);
    assert_eq!(word(asset), num(&v["asset"]));
    assert_eq!(
        word(leaf_of(asset, 12345, owned.npk, owned.rho).unwrap()),
        num(&v["leaf"])
    );
    assert_eq!(
        word(nullifier_of(owned.nk, Fr::from(99u8), 7).unwrap()),
        num(&v["nullifier"])
    );

    let sig = sign(owned.sk, Fr::from(5555u32));
    assert_eq!(word(sig.r8.x), num(&v["signature"]["R8"][0]));
    assert_eq!(word(sig.r8.y), num(&v["signature"]["R8"][1]));
    assert_eq!(scalar_word(sig.s), num(&v["signature"]["S"]));
}

#[test]
fn seals_notes_only_their_recipient_opens() {
    let v = vectors();
    let one = identity(&v["identity1"]["key"]);
    let two = identity(&v["identity2"]["key"]);
    let asset = f(&v["asset"]);
    let ephemeral = secp_scalar(&num(&v["sealed"]["ephemeral"]).to_be_bytes()).unwrap();

    let sealed = seal(two.address(), 777, asset, &ephemeral).unwrap();
    assert_eq!(word(sealed.leaf), num(&v["sealed"]["leaf"]));
    assert_eq!(word(sealed.npk), num(&v["sealed"]["npk"]));
    assert_eq!(word(sealed.rho), num(&v["sealed"]["rho"]));
    assert_eq!(sealed.memo.to_vec(), bytes(&v["sealed"]["memo"]));

    let opened = open(&two, &sealed.memo, sealed.leaf, asset).unwrap();
    assert_eq!(opened.value, 777);
    assert_eq!(opened.rho, sealed.rho);
    assert!(open(&one, &sealed.memo, sealed.leaf, asset).is_none());
    assert!(open(&two, &sealed.memo, sealed.leaf + Fr::from(1u8), asset).is_none());
    assert!(open(&two, &sealed.memo[..64], sealed.leaf, asset).is_none());
    let mut tampered = sealed.memo;
    tampered[40] ^= 1;
    assert!(open(&two, &tampered, sealed.leaf, asset).is_none());

    assert!(seal(two.address(), 1 << 120, asset, &ephemeral).is_err());
}

fn reference_tree(v: &Value) -> IncTree {
    let leaves: Vec<Fr> = v["tree"]["leaves"]
        .as_array()
        .unwrap()
        .iter()
        .map(f)
        .collect();
    let mut tree = IncTree::new();
    for batch in v["tree"]["batches"].as_array().unwrap() {
        let from = batch[0].as_u64().unwrap() as usize;
        let to = batch[1].as_u64().unwrap() as usize;
        let track: Vec<u64> = batch[2]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_u64().unwrap())
            .collect();
        tree.append(&leaves[from..to], &track).unwrap();
    }
    tree
}

#[test]
fn keeps_the_reference_tree() {
    let v = vectors();
    let tree = reference_tree(&v);
    assert_eq!(tree.root(), num(&v["tree"]["root"]));
    assert_eq!(tree.size(), v["tree"]["size"].as_u64().unwrap());

    let state = IncTreeState::from(&tree);
    let json = &v["tree"]["json"];
    let filled: Vec<U256> = json["filled"].as_array().unwrap().iter().map(num).collect();
    assert_eq!(state.filled, filled);
    for (index, path) in json["tracked"].as_object().unwrap() {
        let expected: Vec<U256> = path.as_array().unwrap().iter().map(num).collect();
        assert_eq!(
            state.tracked[&index.parse::<u64>().unwrap()],
            expected,
            "path of {index}"
        );
    }
    let path17: Vec<U256> = tree.path(17).unwrap().iter().copied().map(word).collect();
    let expected: Vec<U256> = v["tree"]["path17"]
        .as_array()
        .unwrap()
        .iter()
        .map(num)
        .collect();
    assert_eq!(path17, expected);
    let insertion: Vec<U256> = tree.insertion_path().iter().copied().map(word).collect();
    let expected: Vec<U256> = v["tree"]["insertionPath"]
        .as_array()
        .unwrap()
        .iter()
        .map(num)
        .collect();
    assert_eq!(insertion, expected);

    // Saved and restored, it keeps growing the same way.
    let mut restored = IncTree::try_from(state).unwrap();
    let mut tree = tree;
    tree.append(&[Fr::from(1u8), Fr::from(2u8)], &[]).unwrap();
    restored
        .append(&[Fr::from(1u8), Fr::from(2u8)], &[])
        .unwrap();
    assert_eq!(restored, tree);
    assert!(tree.path(5).is_err());
    assert!(tree.append(&[Fr::from(1u8)], &[999]).is_err());
}

fn ext_data(v: &Value) -> ExtData {
    ExtData {
        chain_id: v["chainId"].as_str().unwrap().parse().unwrap(),
        pool: v["pool"].as_str().unwrap().parse().unwrap(),
        recipient: v["recipient"].as_str().unwrap().parse().unwrap(),
        ext_amount: v["extAmount"].as_str().unwrap().parse().unwrap(),
        relayer: v["relayer"].as_str().unwrap().parse().unwrap(),
        fee: v["fee"].as_str().unwrap().parse().unwrap(),
        memo0: Bytes::from(bytes(&v["memo0"])),
        memo1: Bytes::from(bytes(&v["memo1"])),
    }
}

#[test]
fn hashes_the_external_data_as_the_pool_does() {
    let v = vectors();
    for case in ["withdrawal", "deposit"] {
        let ext = ext_data(&v["extData"][case]);
        assert_eq!(
            ext.hash().unwrap(),
            num(&v["extData"][case]["hash"]),
            "{case}"
        );
    }
    let mut ext = ext_data(&v["extData"]["deposit"]);
    ext.ext_amount = 1 << 120;
    assert!(ext.hash().is_err());
}

fn check_inputs(got: &kohaku_tacit_circuit::CircuitInputs, expected: &Value) {
    let input = &expected["input"];
    let pairs = |x: &Value| -> Vec<U256> {
        x.as_array()
            .unwrap()
            .iter()
            .flat_map(|p| p.as_array().unwrap().iter().map(num).collect::<Vec<_>>())
            .collect()
    };
    let list = |x: &Value| -> Vec<U256> { x.as_array().unwrap().iter().map(num).collect() };
    assert_eq!(got.root, num(&input["root"]));
    assert_eq!(got.old_root, num(&input["oldRoot"]));
    assert_eq!(got.new_root, num(&input["newRoot"]));
    assert_eq!(got.start_index, num(&input["startIndex"]));
    assert_eq!(got.public_amount, num(&input["publicAmount"]));
    assert_eq!(got.ext_data_hash, num(&input["extDataHash"]));
    assert_eq!(got.asset, num(&input["asset"]));
    assert_eq!(got.nf.to_vec(), list(&input["nf"]));
    assert_eq!(got.out_leaf.to_vec(), list(&input["outLeaf"]));
    assert_eq!(got.in_v.to_vec(), list(&input["inV"]));
    assert_eq!(got.in_rho.to_vec(), list(&input["inRho"]));
    assert_eq!(got.in_nk.to_vec(), list(&input["inNk"]));
    assert_eq!(got.in_ak.concat(), pairs(&input["inAk"]));
    assert_eq!(got.in_index.to_vec(), list(&input["inIndex"]));
    assert_eq!(got.in_path.concat(), pairs(&input["inPath"]));
    assert_eq!(got.sig_r8.concat(), pairs(&input["sigR8"]));
    assert_eq!(got.sig_s.to_vec(), list(&input["sigS"]));
    assert_eq!(got.out_v.to_vec(), list(&input["outV"]));
    assert_eq!(got.out_npk.to_vec(), list(&input["outNpk"]));
    assert_eq!(got.out_rho.to_vec(), list(&input["outRho"]));
    assert_eq!(got.ins_path.to_vec(), list(&input["insPath"]));
    assert_eq!(
        got.public_signals().to_vec(),
        list(&expected["publicSignals"])
    );
}

#[test]
fn builds_the_reference_deposit_witness() {
    let v = vectors();
    let leaves: Vec<Fr> = v["tree"]["leaves"]
        .as_array()
        .unwrap()
        .iter()
        .map(f)
        .collect();
    let mut tree = IncTree::new();
    tree.append(&leaves[..52], &[6]).unwrap();

    let out = OutputNote {
        value: 1000,
        npk: f(&v["sealed"]["npk"]),
        rho: f(&v["sealed"]["rho"]),
    };
    let statement = build(
        f(&v["asset"]),
        &tree,
        [SpendInput::Filler, SpendInput::Filler],
        [Some(out), None],
        1000,
        0,
        f(&v["extData"]["deposit"]["hash"]),
        &mut XcheckRng(0),
    )
    .unwrap();
    check_inputs(&statement.inputs, &v["witnessDeposit"]);

    // A deposit that does not balance.
    assert!(
        build(
            f(&v["asset"]),
            &tree,
            [SpendInput::Filler, SpendInput::Filler],
            [Some(out), None],
            999,
            0,
            Fr::from(1u8),
            &mut XcheckRng(0),
        )
        .is_err()
    );
}

#[test]
fn builds_the_reference_spend_witness() {
    let v = vectors();
    let one = identity(&v["identity1"]["key"]);
    let secret: [u8; 33] = bytes(&v["secret"]).try_into().unwrap();
    let owned = owned_keys(&one.keys, &secret).unwrap();
    let asset = f(&v["asset"]);

    let mut leaves: Vec<Fr> = v["tree"]["leaves"]
        .as_array()
        .unwrap()
        .iter()
        .map(f)
        .collect();
    leaves.truncate(52);
    leaves[6] = leaf_of(asset, 900, owned.npk, owned.rho).unwrap();
    let mut tree = IncTree::new();
    tree.append(&leaves, &[6]).unwrap();

    let out = OutputNote {
        value: 450,
        npk: f(&v["sealed"]["npk"]),
        rho: f(&v["sealed"]["rho"]),
    };
    let mut ext = ext_data(&v["extData"]["withdrawal"]);
    ext.ext_amount = -400;
    ext.fee = 50;
    let note = SpendInput::Note {
        value: 900,
        rho: owned.rho,
        nk: owned.nk,
        sk: owned.sk,
        index: 6,
    };
    let statement = build(
        asset,
        &tree,
        [note, SpendInput::Filler],
        [Some(out), None],
        ext.ext_amount,
        ext.fee,
        ext.hash_fr().unwrap(),
        &mut XcheckRng(0),
    )
    .unwrap();
    check_inputs(&statement.inputs, &v["witnessSpend"]);

    // An input that is not the leaf at its index.
    let mut other = IncTree::new();
    other
        .append(
            &v["tree"]["leaves"].as_array().unwrap()[..52]
                .iter()
                .map(f)
                .collect::<Vec<_>>(),
            &[6],
        )
        .unwrap();
    assert!(
        build(
            asset,
            &other,
            [note, SpendInput::Filler],
            [None, None],
            -900,
            0,
            Fr::from(1u8),
            &mut XcheckRng(0),
        )
        .is_err()
    );
    let _: Scalar = owned.sk;
}

#[test]
fn encodes_the_live_selector_and_topic() {
    use alloy::sol_types::{SolCall, SolEvent};

    use crate::abi::TacitPool;
    assert_eq!(
        TacitPool::Transact::SIGNATURE,
        "Transact(bytes32,bytes32,bytes32,bytes32,uint256,bytes32,address,int256,address,uint256,bytes,bytes)"
    );
    assert_eq!(
        TacitPool::transactCall::SIGNATURE,
        "transact(uint256[2],uint256[2][2],uint256[2],uint256[11],address,int256,address,uint256,bytes,bytes)"
    );
}

#[cfg(feature = "mnemonic")]
#[test]
fn opens_the_account_kohakus_keystore_derives() {
    // Kohaku's MnemonicKeystore (@scure/bip32) at m/29793'/1'/{0,1}' of anvil's mnemonic.
    let phrase = "test test test test test test test test test test test junk";
    for (index, key) in [
        (
            0,
            "dd3bc2f7f6a3589b25b7bf1d2c553b1419d6435a731eb192047b55a6f61a3363",
        ),
        (
            1,
            "405e86abfc4273796bcb42b3f37ea78b888b05d68c092096d0cebe25303c13c1",
        ),
    ] {
        let identity: [u8; 32] = hex::decode(key).unwrap().try_into().unwrap();
        assert_eq!(
            Account::from_mnemonic(phrase, index).unwrap().address(),
            Account::from_identity(&identity).unwrap().address()
        );
    }
}

#[test]
fn debug_output_leaves_out_secrets() {
    let v = vectors();
    let one = identity(&v["identity1"]["key"]);
    let secret: [u8; 33] = bytes(&v["secret"]).try_into().unwrap();
    let note =
        crate::state::OwnedNote::new_for_test(&one, 6, Fr::from(5u8), 900, 1, secret).unwrap();
    let text = format!("{note:?} {one:?}");
    let owned = owned_keys(&one.keys, &secret).unwrap();
    for hidden in [
        scalar_word(owned.sk).to_string(),
        scalar_word(owned.nk).to_string(),
        scalar_word(one.keys.a).to_string(),
        hex::encode(secret),
    ] {
        assert!(!text.contains(&hidden), "{text}");
    }
}
