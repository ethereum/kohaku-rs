#![doc = include_str!("../README.md")]

use alloy::{
    primitives::{Address, B256, Bytes, I256, U256, address, hex, keccak256},
    providers::Provider,
    rpc::types::state::{StateOverride, StateOverridesBuilder},
    sol_types::SolValue,
};
use kohaku_tacit::{
    Payer, ProvenTransaction, SpendError, SyncError, Transaction,
    circuit::{CircuitError, Prover},
    pool_verifier,
};
use kohaku_userop_kit::{
    builder::UserOperationBuilder,
    bundler::{Bundler, BundlerError, BundlerExt},
};
use rand_core::CryptoRng;

use crate::abis::TacitPaymasterData;

mod abis;

/// Added to the quoted gas cost, in basis points.
const FEE_BUFFER_BPS: u128 = 100;

/// The fee the estimate is built with: enough to exercise the pool's fee transfer, small enough
/// that any note covers it. The estimate skips the paymaster's own fee check, so it need not
/// cover the gas.
const SEED_FEE_WEI: u128 = 1;

/// Where the estimate puts a copy of the real verifier, for the forwarder to call.
const SHADOW_VERIFIER: Address = address!("0x00000000000000000000000000004b6f68616b75");

/// `BasePrivacyPaymaster.SIMULATE_SLOT`, which skips the fee check when set.
fn simulate_slot() -> B256 {
    keccak256(b"privacy-paymaster.simulate")
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PaymasterError {
    #[error("the operation's sender is {sender}, but the withdrawal pays {recipient}")]
    SenderIsNotRecipient { sender: Address, recipient: Address },
    #[error("the paymaster takes no withdrawal that inserts notes: withdraw a whole note")]
    InsertsNotes,
    #[error("withdrawing to the paymaster would pay it the whole note")]
    BadRecipient,
    #[error("building the withdrawal failed: {0}")]
    Build(#[from] SpendError),
    #[error("proving failed: {0}")]
    Circuit(#[from] CircuitError),
    #[error("bundler error: {0}")]
    Bundler(#[from] BundlerError),
    #[error("could not read the pool's verifier: {0}")]
    Pool(#[from] SyncError),
    #[error("RPC error: {0}")]
    Transport(#[from] alloy::transports::TransportError),
}

/// Pays for an operation out of a Tacit pool note.
///
/// The paymaster is the withdrawal's relayer, so the pool pays it the fee in the same call, and
/// the rest of the note goes to the recipient, which is this operation's sender. A Tacit proof
/// binds the fee it was made for, so the fee has to be known before the proof exists: the
/// operation is built twice, once with a dummy proof to size the gas, then with the real fee.
pub trait UserOperationPaymasterExt: Sized {
    /// Pays for this operation out of the note the withdrawal `build` returns.
    ///
    /// `build` is called with the payer to use and returns that withdrawal, which must spend a
    /// whole note (no change, so it inserts nothing) and pay this operation's sender. It is
    /// called twice, so it must be able to build the same withdrawal at a different fee, as in
    /// `|payer| wallet.unshield(recipient, note.value - payer.fee, payer, tree, rng)`.
    ///
    /// # Errors
    /// Returns an error if the withdrawal cannot be built at the quoted fee, if it inserts notes
    /// or pays someone other than the sender, or if estimating or proving fails.
    fn with_tacit_paymaster<F, B, R>(
        self,
        paymaster: Address,
        build: F,
        prover: &Prover,
        provider: &impl Provider,
        bundler: &B,
        rng: &mut R,
    ) -> impl Future<Output = Result<Self, PaymasterError>>
    where
        F: FnMut(Payer) -> Result<Transaction, SpendError>,
        B: Bundler + ?Sized,
        R: CryptoRng;
}

impl<S> UserOperationPaymasterExt for UserOperationBuilder<S> {
    #[tracing::instrument(skip_all)]
    async fn with_tacit_paymaster<F, B, R>(
        self,
        paymaster: Address,
        mut build: F,
        prover: &Prover,
        provider: &impl Provider,
        bundler: &B,
        rng: &mut R,
    ) -> Result<Self, PaymasterError>
    where
        F: FnMut(Payer) -> Result<Transaction, SpendError>,
        B: Bundler + ?Sized,
        R: CryptoRng,
    {
        let sender = self.build().user_op.sender;
        let payer = |fee| Payer {
            relayer: paymaster,
            fee,
        };

        let seed = build(payer(SEED_FEE_WEI))?;
        check(&seed, sender, paymaster)?;
        let pool = seed.ext_data().pool;

        // The dummy proof costs what a real one does to verify, so the estimate holds once the
        // real proof replaces it.
        let overrides = overrides(provider, pool, paymaster).await?;
        let builder = self
            .with_paymaster_and_data(paymaster, encode(&seed.prove_dummy()))
            .with_gas_estimate(bundler, Some(&overrides))
            .await?;

        let fee = quoted_fee(&builder);
        let withdrawal = build(payer(fee))?;
        check(&withdrawal, sender, paymaster)?;
        let spend = withdrawal.prove(prover, rng)?;

        Ok(builder.with_paymaster_and_data(paymaster, encode(&spend)))
    }
}

/// What the paymaster will refuse on chain, refused here with a clearer error.
fn check(tx: &Transaction, sender: Address, paymaster: Address) -> Result<(), PaymasterError> {
    if tx.inserts() {
        return Err(PaymasterError::InsertsNotes);
    }
    let recipient = tx.ext_data().recipient;
    if recipient != sender {
        return Err(PaymasterError::SenderIsNotRecipient { sender, recipient });
    }
    if recipient == paymaster {
        return Err(PaymasterError::BadRecipient);
    }
    Ok(())
}

/// The most this operation can cost, plus a buffer for the gas price moving before it lands.
fn quoted_fee<S>(builder: &UserOperationBuilder<S>) -> u128 {
    let op = builder.build();
    let cost = op
        .total_gas_limit()
        .saturating_mul(op.user_op.max_fee_per_gas);
    cost.saturating_add(cost / 10_000 * FEE_BUFFER_BPS)
}

/// The state an estimate runs against: a verifier that takes the dummy proof, and the
/// paymaster's fee check switched off so the seed fee need not cover the gas.
async fn overrides(
    provider: &impl Provider,
    pool: Address,
    paymaster: Address,
) -> Result<StateOverride, PaymasterError> {
    let verifier = pool_verifier(provider, pool).await?;
    let code = provider.get_code_at(verifier).await?;

    Ok(StateOverridesBuilder::default()
        .with_code(SHADOW_VERIFIER, code)
        .with_code(verifier, forwarder(SHADOW_VERIFIER))
        .with_state_diff(paymaster, [(simulate_slot(), B256::with_last_byte(1))])
        .build())
}

/// Code that calls `target`, ignores the outcome, and returns `true`.
fn forwarder(target: Address) -> Bytes {
    [
        // calldatacopy(0, 0, calldatasize); staticcall(gas, target, 0, calldatasize, 0, 0)
        &hex!("3660006000376000600036600073")[..],
        target.as_slice(),
        // drop the outcome; mstore(0, 1); return(0, 32)
        &hex!("5afa50600160005260206000f3"),
    ]
    .concat()
    .into()
}

/// `TacitPaymaster.PaymasterData` for a proven withdrawal.
fn encode(proven: &ProvenTransaction) -> Bytes {
    let tx = proven.transaction();
    let ext = tx.ext_data();
    let proof = proven.proof();

    TacitPaymasterData {
        pool: ext.pool,
        pA: proof.pa(),
        pB: proof.pb(),
        pC: proof.pc(),
        publicInputs: tx.inputs().public_signals(),
        recipient: ext.recipient,
        extAmount: i256(ext.ext_amount),
        fee: U256::from(ext.fee),
    }
    .abi_encode()
    .into()
}

fn i256(x: i128) -> I256 {
    let magnitude = I256::from_raw(U256::from(x.unsigned_abs()));
    if x < 0 { -magnitude } else { magnitude }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same sample as `test_sdkEncodingMatches` in the contract's Foundry tests and
    /// `paymaster-data.test.ts` in its SDK, so the three encoders cannot drift apart.
    #[test]
    fn encodes_paymaster_data_as_the_contract_decodes_it() {
        let data = TacitPaymasterData {
            pool: address!("0x000000c2A20657CE25f2Ba99737933D031AFBEE9"),
            pA: [U256::from(1), U256::from(2)],
            pB: [
                [U256::from(3), U256::from(4)],
                [U256::from(5), U256::from(6)],
            ],
            pC: [U256::from(7), U256::from(8)],
            publicInputs: std::array::from_fn(|i| {
                if i < 9 {
                    U256::from(10 + i)
                } else {
                    U256::ZERO
                }
            }),
            recipient: address!("0x0000000000000000000000000000000005A71cE2"),
            extAmount: i256(-900_000_000_000_000_000),
            fee: U256::from(100_000_000_000_000_000_u128),
        };

        let expected = include_str!("../tests/fixtures/paymaster-data.hex").trim();
        assert_eq!(hex::encode(data.abi_encode()), expected);
    }

    #[test]
    fn signs_the_external_amount() {
        assert_eq!(i256(-1), I256::MINUS_ONE);
        assert_eq!(i256(0), I256::ZERO);
        assert_eq!(i256(7), I256::try_from(7).unwrap());
    }

    #[test]
    fn the_simulate_slot_is_the_contracts() {
        // keccak256("privacy-paymaster.simulate")
        assert_eq!(
            simulate_slot().to_string(),
            "0x145e9943c0b3dac8c63f9b631de8c7dc16cbf1eda76f5787bf9534546af02aec"
        );
    }
}
