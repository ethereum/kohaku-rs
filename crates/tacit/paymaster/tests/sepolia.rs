//! Against the pool and paymaster deployed on Sepolia.
//!
//! Needs a funded key, an RPC endpoint and the ceremony artifacts, so it is `#[ignore]`d and skips
//! with a message when they are missing:
//!
//! ```sh
//! SEPOLIA_RPC_URL=… TACIT_DEPLOY_KEY=0x… \
//!   TACIT_ARTIFACTS_DIR=<dir with transact.wasm, transact_final.zkey> \
//!   cargo test --release -p kohaku-tacit-paymaster --test sepolia -- --ignored --nocapture
//! ```

use std::{path::PathBuf, time::Duration};

use alloy::{
    primitives::{Address, U256, address},
    providers::{DynProvider, Provider, ProviderBuilder},
    signers::local::PrivateKeySigner,
};
use kohaku_tacit::{Account, Pool, Wallet, circuit::Prover};

/// The pool, at the same address as on Ethereum.
const POOL: Address = address!("0x000000c2A20657CE25f2Ba99737933D031AFBEE9");
const POOL_BLOCK: u64 = 12_127_764;
/// `TacitPaymaster`, deployed through CreateX so the address is the same on every chain.
pub const PAYMASTER: Address = address!("0xAC99d6ef0DFc4c1B9e6D5A1862aCc0a1646E9BD0");

/// The note this test keeps on Sepolia, so a rerun spends what the last run shielded.
const IDENTITY: [u8; 32] = [0x5e; 32];
const SHIELD_WEI: u128 = 20_000_000_000_000_000; // 0.02 ETH

/// What the test needs from the environment, or `None` to skip.
struct Env {
    provider: DynProvider,
    prover: Prover,
}

fn env() -> Option<Env> {
    let (Ok(rpc), Ok(key), Ok(dir)) = (
        std::env::var("SEPOLIA_RPC_URL"),
        std::env::var("TACIT_DEPLOY_KEY"),
        std::env::var("TACIT_ARTIFACTS_DIR"),
    ) else {
        eprintln!("skipped: set SEPOLIA_RPC_URL, TACIT_DEPLOY_KEY and TACIT_ARTIFACTS_DIR");
        return None;
    };
    let signer: PrivateKeySigner = key.trim().parse().expect("a private key");
    let provider = ProviderBuilder::new()
        .wallet(signer)
        .connect_http(rpc.parse().expect("an RPC url"))
        .erased();
    let dir = PathBuf::from(dir);
    let prover = Prover::new(
        &std::fs::read(dir.join("transact.wasm")).expect("transact.wasm"),
        &std::fs::read(dir.join("transact_final.zkey")).expect("transact_final.zkey"),
    )
    .expect("the ceremony artifacts");
    Some(Env { provider, prover })
}

fn wallet() -> Wallet {
    let pool = Pool::new(11_155_111, POOL, POOL_BLOCK)
        .with_confirmations(1)
        .with_log_chunk(50_000);
    Wallet::new(
        Account::from_identity(&IDENTITY).expect("an account"),
        pool.with_relayer(None),
    )
}

/// The public endpoints are load balanced and answer from stale backends, so a read that matters
/// is taken more than once.
async fn settle(provider: &DynProvider) {
    for _ in 0..3 {
        tokio::time::sleep(Duration::from_secs(3)).await;
        let _ = provider.get_block_number().await;
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a funded Sepolia key and the ceremony artifacts"]
async fn shields_a_note_on_sepolia() {
    let Some(Env { provider, prover }) = env() else {
        return;
    };
    let mut wallet = wallet();
    let mut rng = rand::rng();

    wallet.sync(&provider).await.expect("sync");
    println!(
        "address {} holds {} wei in {} notes",
        wallet.address(),
        wallet.balance(),
        wallet.notes().count()
    );

    if wallet.balance() >= SHIELD_WEI {
        println!("a note is already there, nothing to shield");
        return;
    }

    let tx = wallet.shield(SHIELD_WEI, None, &mut rng).expect("build");
    println!("proving the shield…");
    let call = tx.prove(&prover, &mut rng).expect("prove").call();
    let receipt = provider
        .send_transaction(call.into())
        .await
        .expect("send")
        .get_receipt()
        .await
        .expect("receipt");
    assert!(receipt.status(), "the shield reverted");
    println!(
        "shielded in block {} using {} gas, tx {}",
        receipt.block_number.unwrap_or_default(),
        receipt.gas_used,
        receipt.transaction_hash
    );

    settle(&provider).await;
    wallet.sync(&provider).await.expect("resync");
    assert!(wallet.balance() >= SHIELD_WEI, "the note was not found");
    assert_eq!(
        provider.get_balance(POOL).await.expect("pool balance") >= U256::from(SHIELD_WEI),
        true
    );
    println!(
        "the wallet found {} wei in {} notes",
        wallet.balance(),
        wallet.notes().count()
    );
}
