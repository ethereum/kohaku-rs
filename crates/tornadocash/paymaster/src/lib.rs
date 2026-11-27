#![doc = include_str!("../README.md")]

use alloy::{
    primitives::{Address, B256, Bytes, U256, address, hex, keccak256},
    providers::Provider,
    rpc::types::state::{StateOverride, StateOverridesBuilder},
    sol_types::SolValue,
};
use kohaku_tornadocash::{
    Asset, Payer, Pool, ProvenWithdrawal, Withdrawal, WithdrawalError, merkle_tree::MerkleProof,
};
use kohaku_tornadocash_circuit::CircuitArtifacts;
use kohaku_tornadocash_provider::TornadoProviderExt;
use kohaku_userop_kit::{
    SignableUserOperation, UserOperationRequest,
    bundler::{BundlerApi, PimlicoApi, PrepareUserOperationError},
    smart_account::SmartAccount,
};

use crate::abis::{TornadoPaymaster, TornadoPaymasterData};

mod abis;

/// Arbitrary mocked address a pool's verifier is moved to during gas estimation.
const SHADOW_VERIFIER: Address = address!("0x00000000000000000000000000004b6f68616b75");

/// Preimage of the paymaster's `SIMULATE_SLOT`, which skips its fee check when set.
const SIMULATE_SLOT_KEY: &[u8] = b"privacy-paymaster.simulate";

/// Extension trait for bundlers that prepares withdrawals paid for with tornadocash notes.
///
/// Tornadocash paymasters use a shielded note to pay for a `UserOp`'s gas. The remainder of the
/// note is withdrawn to the recipient.
pub trait TornadoPaymasterExt: BundlerApi + PimlicoApi {
    /// Prepares an operation from `account` that withdraws `withdrawal`'s note.
    ///
    /// Gas is paid out of the note.
    ///
    /// Returns the operation and the fee in the pool's asset. The recipient receives
    /// `denomination - fee`.
    ///
    /// # Errors
    /// Returns an error if the pool has no paymaster, if the note cannot cover the operation's
    /// gas, or if estimation, the fee quote, proving or filling fails.
    fn prepare_tornado_withdrawal<S, R>(
        &self,
        account: &S,
        withdrawal: Withdrawal,
        merkle_proof: &MerkleProof,
        provider: &impl Provider,
        circuit_artifacts: &CircuitArtifacts,
        rng: &mut R,
    ) -> impl std::future::Future<Output = Result<(SignableUserOperation, U256), PaymasterError>>
    where
        S: SmartAccount,
        R: rand::CryptoRng;
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PaymasterError {
    #[error("Pool missing paymaster: {0}")]
    PoolMissingPaymaster(Pool),
    #[error("Pool denomination cannot cover the operation's gas: {required} > {denomination}")]
    InsufficientDenomination { required: U256, denomination: U256 },
    #[error("Provider error: {0}")]
    Provider(#[from] alloy::contract::Error),
    #[error("Transport error: {0}")]
    Transport(#[from] alloy::transports::TransportError),
    #[error("Prepare error: {0}")]
    Prepare(#[from] PrepareUserOperationError),
    #[error("Withdrawal error: {0}")]
    Withdrawal(#[from] WithdrawalError),
}

impl<B: BundlerApi + PimlicoApi + ?Sized> TornadoPaymasterExt for B {
    #[tracing::instrument(skip_all)]
    async fn prepare_tornado_withdrawal<S, R>(
        &self,
        account: &S,
        withdrawal: Withdrawal,
        merkle_proof: &MerkleProof,
        provider: &impl Provider,
        circuit_artifacts: &CircuitArtifacts,
        rng: &mut R,
    ) -> Result<(SignableUserOperation, U256), PaymasterError>
    where
        S: SmartAccount,
        R: rand::CryptoRng,
    {
        let pool = &withdrawal.pool;
        let paymaster = paymaster_address(pool)?;
        let denomination = U256::from(pool.amount_wei);

        let fees = self.get_user_operation_gas_price().await?.slow;
        let request = UserOperationRequest::<S::Call>::default()
            .with_max_fee_per_gas(fees.max_fee_per_gas)
            .with_max_priority_fee_per_gas(fees.max_priority_fee_per_gas);

        //? Leaving the recipient 1 wei makes both of the pool's transfers and `postOp`'s refund
        //? nonzero, like a real withdrawal's.
        let placeholder_fee = denomination - U256::from(1);
        let placeholder_data =
            placeholder_paymaster_data(&withdrawal, paymaster, placeholder_fee, merkle_proof);
        let estimate = self
            .prepare_user_operation(
                account,
                request
                    .clone()
                    .with_paymaster(paymaster, placeholder_data)
                    .with_state_override(estimation_overrides(provider, pool, paymaster).await?),
            )
            .await?;

        let max_cost = U256::from(estimate.total_gas_limit()) * U256::from(fees.max_fee_per_gas);
        let fee = quote_in_fee_token(provider, pool, paymaster, max_cost).await?;
        if fee > denomination {
            return Err(PaymasterError::InsufficientDenomination {
                required: fee,
                denomination,
            });
        }

        let proven = withdrawal
            .with_payer(Payer {
                address: paymaster,
                fee,
                refund: U256::ZERO,
            })
            .prove(merkle_proof, circuit_artifacts, rng)?;
        let request = request
            .with_gas_limits(estimate.user_op.gas_limits())
            .with_paymaster(paymaster, encode_paymaster_data(&proven));
        let op = self.prepare_user_operation(account, request).await?;

        Ok((op, fee))
    }
}

fn paymaster_address(pool: &Pool) -> Result<Address, PaymasterError> {
    pool.paymaster
        .ok_or_else(|| PaymasterError::PoolMissingPaymaster(pool.clone()))
}

/// Gas estimation state override.
///
/// Overrides the pool's verifier to accept any proof and sets the paymaster's
/// simulation flag. State overrides increase estimated gas costs by ~3k.
async fn estimation_overrides(
    provider: &impl Provider,
    pool: &Pool,
    paymaster: Address,
) -> Result<StateOverride, PaymasterError> {
    let verifier = provider.verifier(pool).await?;
    let code = provider.get_code_at(verifier).await?;

    Ok(StateOverridesBuilder::default()
        .with_code(SHADOW_VERIFIER, code)
        .with_code(verifier, forwarder(SHADOW_VERIFIER))
        .with_state_diff(
            paymaster,
            [(keccak256(SIMULATE_SLOT_KEY), B256::with_last_byte(1))],
        )
        .build())
}

/// Paymaster data for a placeholder `withdrawal` proof.
fn placeholder_paymaster_data(
    withdrawal: &Withdrawal,
    paymaster: Address,
    fee: U256,
    merkle_proof: &MerkleProof,
) -> Bytes {
    let placeholder = withdrawal
        .clone()
        .with_payer(Payer {
            address: paymaster,
            fee,
            refund: U256::ZERO,
        })
        .prove_dummy(merkle_proof);
    encode_paymaster_data(&placeholder)
}

/// Quotes a gas cost in wei in the pool's fee token, including the paymaster's markup.
async fn quote_in_fee_token(
    provider: &impl Provider,
    pool: &Pool,
    paymaster: Address,
    cost: U256,
) -> Result<U256, PaymasterError> {
    let token = match pool.asset {
        Asset::Native { .. } => Address::ZERO,
        Asset::Erc20 { address, .. } => address,
    };

    Ok(TornadoPaymaster::new(paymaster, provider)
        .quoteWeiInToken(token, cost)
        .call()
        .await?)
}

fn encode_paymaster_data(withdrawal: &ProvenWithdrawal) -> Bytes {
    let data = TornadoPaymasterData {
        instance: withdrawal.pool().address,
        proof: withdrawal.solidity_proof(),
        root: withdrawal.root().into(),
        nullifierHash: withdrawal.note().nullifier_hash().into(),
        recipient: withdrawal.recipient(),
        fee: withdrawal.payer().fee,
    };

    data.abi_encode().into()
}

/// Execution code that calls `target`, discards the outcome, and returns `true`.
fn forwarder(target: Address) -> Bytes {
    [
        // calldatacopy(0, 0, calldatasize)
        // staticcall(gas, target, 0, calldatasize, 0, 0)
        &hex!("3660006000376000600036600073")[..],
        target.as_slice(),
        // pop the call's success
        // mstore(0, 1)
        // return(0, 32)
        &hex!("5afa50600160005260206000f3"),
    ]
    .concat()
    .into()
}
