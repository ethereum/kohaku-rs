//! End to end against the pool's own bytecode on a local anvil: every operation is built, proven,
//! sent and read back by the wallets that own the notes.
//!
//! The fixture's verifier and pool are the live mainnet contracts' code (identical runtime, apart
//! from immutables). Proving needs the ceremony artifacts: point `TACIT_ARTIFACTS_DIR` at a
//! directory holding `transact.wasm` and `transact_final.zkey`, put `anvil` on the PATH, and run
//! with `cargo test --release -p kohaku-tacit --test pool -- --ignored`.

use std::{path::PathBuf, sync::OnceLock};

use alloy::{
    network::TransactionBuilder,
    primitives::{Address, Bytes, U256},
    providers::{DynProvider, Provider, ProviderBuilder},
    rpc::types::TransactionRequest,
    sol_types::SolValue,
};
use kohaku_tacit::{
    Account, Call, Kind, Payer, Pool, ProvenTransaction, SavedState, Wallet, circuit::Prover,
};

const ETH: u128 = 1_000_000_000_000_000_000;

/// The ceremony files are about 33 MB and not part of the repo: without them these tests skip.
fn artifacts_missing() -> bool {
    let missing = std::env::var_os("TACIT_ARTIFACTS_DIR").is_none();
    if missing {
        eprintln!(
            "skipped: set TACIT_ARTIFACTS_DIR to a directory with transact.wasm and \
             transact_final.zkey"
        );
    }
    missing
}

fn prover() -> &'static Prover {
    static PROVER: OnceLock<Prover> = OnceLock::new();
    PROVER.get_or_init(|| {
        let dir = PathBuf::from(std::env::var("TACIT_ARTIFACTS_DIR").expect("TACIT_ARTIFACTS_DIR"));
        let wasm = std::fs::read(dir.join("transact.wasm")).unwrap();
        let zkey = std::fs::read(dir.join("transact_final.zkey")).unwrap();
        Prover::new(&wasm, &zkey).unwrap()
    })
}

fn bytecode(name: &str) -> Bytes {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/tacit_pool.json")).unwrap();
    fixture[name]["bytecode"].as_str().unwrap().parse().unwrap()
}

async fn deploy(provider: &DynProvider, code: Bytes) -> (Address, u64) {
    let receipt = provider
        .send_transaction(TransactionRequest::default().with_deploy_code(code))
        .await
        .unwrap()
        .get_receipt()
        .await
        .unwrap();
    assert!(receipt.status());
    (
        receipt.contract_address.unwrap(),
        receipt.block_number.unwrap(),
    )
}

/// A fresh anvil with the verifier and an ETH pool deployed.
async fn setup() -> (DynProvider, Pool) {
    let provider = ProviderBuilder::new().connect_anvil_with_wallet().erased();
    let (verifier, _) = deploy(&provider, bytecode("verifier")).await;
    let mut code = bytecode("pool").to_vec();
    code.extend((verifier, Address::ZERO).abi_encode_params());
    let (pool, block) = deploy(&provider, code.into()).await;
    let chain_id = provider.get_chain_id().await.unwrap();
    let pool = Pool::new(chain_id, pool, block)
        .with_confirmations(0)
        .with_log_chunk(500);
    (provider, pool)
}

async fn send(provider: &DynProvider, call: Call) -> Result<(), String> {
    match provider.send_transaction(call.into()).await {
        Ok(pending) => {
            let receipt = pending.get_receipt().await.map_err(|e| e.to_string())?;
            if receipt.status() {
                Ok(())
            } else {
                Err("reverted".into())
            }
        }
        Err(e) => Err(e.to_string()),
    }
}

/// Whether `call` would revert, checked with `eth_call` so the sender's nonce is not consumed.
async fn reverts(provider: &DynProvider, call: Call) -> bool {
    provider.call(call.into()).await.is_err()
}

fn prove(tx: kohaku_tacit::Transaction) -> ProvenTransaction {
    tx.prove(prover(), &mut rand::rng()).unwrap()
}

fn wallet(seed: u8, pool: &Pool) -> Wallet {
    Wallet::new(Account::from_identity(&[seed; 32]).unwrap(), pool.clone())
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the ceremony artifacts and anvil; run with --release"]
async fn shields_pays_and_withdraws() {
    if artifacts_missing() {
        return;
    }
    let (provider, pool) = setup().await;
    let mut rng = rand::rng();
    let mut alice = wallet(0x11, &pool);
    let mut bob = wallet(0x22, &pool);
    // Bob keeps history only three blocks deep; what is newer he sees through his view.
    bob = Wallet::new(bob.account().clone(), pool.clone().with_confirmations(3));
    let relayer = Address::with_last_byte(0xee);
    let outside = Address::with_last_byte(0xcc);

    // Alice shields one ETH for herself, and 0.3 ETH straight to Bob's address.
    alice.sync(&provider).await.unwrap();
    let tx = alice.shield(ETH, None, &mut rng).unwrap();
    assert_eq!(tx.kind(), Kind::Shield);
    send(&provider, prove(tx).call()).await.unwrap();
    alice.sync(&provider).await.unwrap();
    let tx = alice
        .shield(3 * ETH / 10, Some(bob.address()), &mut rng)
        .unwrap();
    send(&provider, prove(tx).call()).await.unwrap();

    alice.sync(&provider).await.unwrap();
    bob.sync(&provider).await.unwrap();
    assert_eq!(alice.balance(), ETH);
    assert_eq!(bob.balance(), 3 * ETH / 10);
    assert_eq!(
        provider.get_balance(pool.address).await.unwrap(),
        U256::from(13 * ETH / 10)
    );

    // Alice pays Bob 0.25 ETH; her own account sends it.
    let tx = alice
        .transfer(bob.address(), ETH / 4, Payer::SELF, alice.tree(), &mut rng)
        .unwrap();
    send(&provider, prove(tx).call()).await.unwrap();
    alice.sync(&provider).await.unwrap();
    bob.sync(&provider).await.unwrap();
    assert_eq!(alice.balance(), 3 * ETH / 4);
    assert_eq!(bob.balance(), 55 * ETH / 100);
    assert_eq!(
        alice.notes().count(),
        1,
        "the spent note is gone, the change is kept"
    );

    // Bob withdraws 0.5 ETH to a public address through a relayer, which is paid 0.001 ETH from the
    // pool. Anyone may send the relayed transaction.
    let fee = ETH / 1000;
    let payer = Payer { relayer, fee };
    let tx = bob
        .unshield(outside, ETH / 2, payer, bob.tree(), &mut rng)
        .unwrap();
    assert!(tx.inserts(), "the change is a new note");
    let proven = prove(tx);
    let request = proven.relay_request();
    assert_eq!(request.fee, fee.to_string());
    assert_eq!(request.ext_amount, (-(ETH.cast_signed() / 2)).to_string());
    send(&provider, proven.call()).await.unwrap();
    assert_eq!(
        provider.get_balance(outside).await.unwrap(),
        U256::from(ETH / 2)
    );
    assert_eq!(
        provider.get_balance(relayer).await.unwrap(),
        U256::from(fee)
    );

    // The same proof again: its nullifiers are spent.
    assert!(reverts(&provider, proven.call()).await);

    bob.sync(&provider).await.unwrap();
    assert_eq!(bob.balance(), 55 * ETH / 100 - ETH / 2 - fee);

    // Saved and restored, a wallet resumes where it stopped.
    let saved: SavedState =
        serde_json::from_str(&serde_json::to_string(&alice.save()).unwrap()).unwrap();
    let mut restored = Wallet::restore(alice.account().clone(), pool.clone(), saved).unwrap();
    restored.sync(&provider).await.unwrap();
    alice.sync(&provider).await.unwrap();
    assert_eq!(restored.balance(), alice.balance());
    assert_eq!(restored.tree().root(), alice.tree().root());
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the ceremony artifacts and anvil; run with --release"]
async fn consolidates_and_withdraws_without_change() {
    if artifacts_missing() {
        return;
    }
    let (provider, pool) = setup().await;
    let mut rng = rand::rng();
    let mut carol = wallet(0x33, &pool);
    let mut dave = wallet(0x44, &pool);

    for amount in [ETH / 10, ETH / 5, ETH / 20] {
        carol.sync(&provider).await.unwrap();
        let tx = carol.shield(amount, None, &mut rng).unwrap();
        send(&provider, prove(tx).call()).await.unwrap();
    }
    carol.sync(&provider).await.unwrap();
    assert_eq!(carol.notes().count(), 3);

    // 0.34 ETH needs all three notes; one transaction takes two.
    let error = carol
        .transfer(
            dave.address(),
            34 * ETH / 100,
            Payer::SELF,
            carol.tree(),
            &mut rng,
        )
        .unwrap_err();
    assert!(error.to_string().contains("consolidate"), "{error}");
    let tx = carol
        .consolidate(Payer::SELF, carol.tree(), &mut rng)
        .unwrap();
    assert_eq!(tx.amount(), 3 * ETH / 10);
    send(&provider, prove(tx).call()).await.unwrap();
    carol.sync(&provider).await.unwrap();
    assert_eq!(carol.notes().count(), 2);
    assert_eq!(carol.balance(), 35 * ETH / 100);
    // A spend built against a head that has since moved is refused (StaleRoot): rebuild it.
    let first = carol
        .transfer(
            dave.address(),
            ETH / 100,
            Payer::SELF,
            carol.tree(),
            &mut rng,
        )
        .unwrap();
    dave.sync(&provider).await.unwrap();
    let other = dave.shield(ETH / 100, None, &mut rng).unwrap();
    send(&provider, prove(other).call()).await.unwrap();
    assert!(reverts(&provider, prove(first).call()).await);
    carol.sync(&provider).await.unwrap();
    // Withdrawing a whole note leaves no change: nothing is inserted, so the proof does not depend
    // on the head and lands after another transaction.
    let note = carol.notes().map(|n| n.value).min().unwrap();
    let tx = carol
        .unshield(
            Address::with_last_byte(0xdd),
            note,
            Payer::SELF,
            carol.tree(),
            &mut rng,
        )
        .unwrap();
    assert!(!tx.inserts());
    let proven = prove(tx);
    dave.sync(&provider).await.unwrap();
    let other = dave.shield(ETH / 100, None, &mut rng).unwrap();
    send(&provider, prove(other).call()).await.unwrap();
    send(&provider, proven.call()).await.unwrap();
    assert_eq!(
        provider
            .get_balance(Address::with_last_byte(0xdd))
            .await
            .unwrap(),
        U256::from(note)
    );
    carol.sync(&provider).await.unwrap();
    assert_eq!(carol.balance(), 35 * ETH / 100 - note);
    dave.sync(&provider).await.unwrap();
    assert_eq!(dave.balance(), 2 * ETH / 100);
}
