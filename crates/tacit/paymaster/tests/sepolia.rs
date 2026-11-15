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

mod operation {
    use alloy::{
        dyn_abi::Eip712Domain,
        primitives::{B256, Bytes, U256, Uint},
        providers::Provider,
        signers::local::PrivateKeySigner,
        sol,
        sol_types::SolCall,
    };
    use kohaku_tacit::Payer;
    use kohaku_tacit_paymaster::UserOperationPaymasterExt;
    use kohaku_userop_kit::{
        builder::UserOperationBuilder,
        bundler::{Bundler, BundlerError},
        entry_point::ENTRY_POINT_08,
        signable_user_operation::SignableUserOperation,
        signed_user_operation::SignedUserOperation,
        user_operation::{
            UserOperation, UserOperationGasEstimate, UserOperationHash, UserOperationReceipt,
        },
    };

    use super::*;

    sol! {
        #[derive(Debug)]
        struct PackedUserOperation {
            address sender;
            uint256 nonce;
            bytes initCode;
            bytes callData;
            bytes32 accountGasLimits;
            uint256 preVerificationGas;
            bytes32 gasFees;
            bytes paymasterAndData;
            bytes signature;
        }

        #[sol(rpc)]
        interface IEntryPoint {
            function handleOps(PackedUserOperation[] ops, address beneficiary) external;
            function getNonce(address sender, uint192 key) external view returns (uint256);
            function balanceOf(address account) external view returns (uint256);
        }
    }

    /// The EIP-712 domain of EntryPoint v0.8, which the kit keeps private.
    fn domain(chain_id: u64) -> Eip712Domain {
        alloy::sol_types::eip712_domain! {
            name: "ERC4337",
            version: "1",
            chain_id: chain_id,
            verifying_contract: ENTRY_POINT_08,
        }
    }

    /// A 4337 account that accepts any operation, deployed for this test.
    const ACCOUNT: Address = address!("0x1bDCD2A09a97Be8E0065B7eb46e9511613E48762");

    /// Gas limits in place of a bundler's `eth_estimateUserOperationGas`. The paymaster refunds
    /// whatever the operation does not spend, so erring high costs the note nothing.
    struct FixedGas {
        max_fee_per_gas: u128,
    }

    #[async_trait::async_trait]
    impl Bundler for FixedGas {
        async fn estimate_gas(
            &self,
            _op: &SignableUserOperation,
            _overrides: Option<&alloy::rpc::types::state::StateOverride>,
        ) -> Result<UserOperationGasEstimate, BundlerError> {
            Ok(UserOperationGasEstimate {
                pre_verification_gas: 200_000,
                verification_gas_limit: 150_000,
                call_gas_limit: 0,
                paymaster_verification_gas_limit: Some(1_200_000),
                paymaster_post_op_gas_limit: Some(100_000),
                max_fee_per_gas: self.max_fee_per_gas,
                max_priority_fee_per_gas: self.max_fee_per_gas / 2,
            })
        }

        async fn send_user_operation(
            &self,
            _op: &SignedUserOperation,
        ) -> Result<UserOperationHash, BundlerError> {
            unimplemented!("this test submits through the EntryPoint itself")
        }

        async fn receipt(
            &self,
            _hash: UserOperationHash,
        ) -> Result<Option<UserOperationReceipt>, BundlerError> {
            unimplemented!()
        }

        async fn wait_for_receipt(
            &self,
            _hash: UserOperationHash,
        ) -> Result<UserOperationReceipt, BundlerError> {
            unimplemented!()
        }
    }

    fn pack(op: &UserOperation) -> PackedUserOperation {
        let two = |hi: u128, lo: u128| {
            let mut out = [0u8; 32];
            out[..16].copy_from_slice(&hi.to_be_bytes());
            out[16..].copy_from_slice(&lo.to_be_bytes());
            B256::from(out)
        };
        let mut paymaster_and_data = Vec::new();
        if let Some(paymaster) = op.paymaster {
            paymaster_and_data.extend_from_slice(paymaster.as_slice());
            paymaster_and_data.extend_from_slice(
                &op.paymaster_verification_gas_limit
                    .unwrap_or(0)
                    .to_be_bytes(),
            );
            paymaster_and_data
                .extend_from_slice(&op.paymaster_post_op_gas_limit.unwrap_or(0).to_be_bytes());
            if let Some(data) = &op.paymaster_data {
                paymaster_and_data.extend_from_slice(data);
            }
        }
        PackedUserOperation {
            sender: op.sender,
            nonce: op.nonce,
            initCode: Bytes::new(),
            callData: op.call_data.clone(),
            accountGasLimits: two(op.verification_gas_limit, op.call_gas_limit),
            preVerificationGas: U256::from(op.pre_verification_gas),
            gasFees: two(op.max_priority_fee_per_gas, op.max_fee_per_gas),
            paymasterAndData: paymaster_and_data.into(),
            signature: op.signature.clone(),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs a funded Sepolia key, the ceremony artifacts and a shielded note"]
    async fn a_note_pays_for_its_own_withdrawal_on_sepolia() {
        let Some(Env { provider, prover }) = env() else {
            return;
        };
        let signer: PrivateKeySigner = std::env::var("TACIT_DEPLOY_KEY")
            .expect("key")
            .trim()
            .parse()
            .expect("a private key");
        let mut wallet = wallet();
        let mut rng = rand::rng();

        wallet.sync(&provider).await.expect("sync");
        let note = wallet
            .notes()
            .map(|n| n.value)
            .max()
            .expect("shield a note first with shields_a_note_on_sepolia");
        println!("spending a note of {note} wei");

        let entry_point = IEntryPoint::new(ENTRY_POINT_08, &provider);
        let nonce = entry_point
            .getNonce(ACCOUNT, Uint::from(0))
            .call()
            .await
            .expect("nonce");
        let before = (
            provider
                .get_balance(ACCOUNT)
                .await
                .expect("account balance"),
            entry_point
                .balanceOf(PAYMASTER)
                .call()
                .await
                .expect("paymaster deposit"),
        );

        let bundler = FixedGas {
            max_fee_per_gas: 2_000_000_000,
        };
        let builder = UserOperationBuilder::<()>::new(ACCOUNT, ENTRY_POINT_08, domain(11_155_111))
            .with_nonce(nonce);

        println!("building and proving the withdrawal…");
        let builder = builder
            .with_tacit_paymaster(
                PAYMASTER,
                |payer: Payer| {
                    wallet.unshield(
                        ACCOUNT,
                        note - payer.fee,
                        payer,
                        wallet.tree(),
                        &mut rand::rng(),
                    )
                },
                &prover,
                &provider,
                &bundler,
                &mut rng,
            )
            .await
            .expect("the paymaster path");

        let signed = builder.build().sign(&signer).await.expect("sign");
        let op = pack(&signed.user_op);
        println!(
            "fee bound into the proof: {} wei",
            U256::from_be_slice(&op.paymasterAndData[op.paymasterAndData.len() - 32..])
        );

        let receipt = provider
            .send_transaction(
                alloy::rpc::types::TransactionRequest::default()
                    .to(ENTRY_POINT_08)
                    .input(
                        IEntryPoint::handleOpsCall {
                            ops: vec![op],
                            beneficiary: signer.address(),
                        }
                        .abi_encode()
                        .into(),
                    )
                    .gas_limit(6_000_000),
            )
            .await
            .expect("send handleOps")
            .get_receipt()
            .await
            .expect("receipt");
        assert!(receipt.status(), "handleOps reverted: {receipt:?}");
        println!(
            "landed in block {} using {} gas, tx {}",
            receipt.block_number.unwrap_or_default(),
            receipt.gas_used,
            receipt.transaction_hash
        );

        settle(&provider).await;
        let after = (
            provider
                .get_balance(ACCOUNT)
                .await
                .expect("account balance"),
            entry_point
                .balanceOf(PAYMASTER)
                .call()
                .await
                .expect("paymaster deposit"),
        );
        let paid = after.0 - before.0;
        let spent = before.1 - after.1;
        println!("the account received {paid} wei; the paymaster's deposit fell by {spent} wei");
        assert!(paid > U256::ZERO, "the account was not paid");
        assert!(spent > U256::ZERO, "the gas was not paid from the deposit");
        assert!(
            paid > U256::from(note) - U256::from(note / 10),
            "most of the note arrived"
        );

        wallet.sync(&provider).await.expect("resync");
        println!("the wallet now holds {} wei", wallet.balance());
    }
}
