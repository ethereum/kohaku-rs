#![doc = include_str!("../README.md")]

use alloy::{
    primitives::{Address, Bytes, U256, address, hex},
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
    UserOperationRequest,
    bundler::{BundlerApi, PimlicoApi, PrepareUserOperationError},
    smart_account::SmartAccount,
};

use crate::abis::{TornadoPaymaster, TornadoPaymasterData};

mod abis;

/// Safety margin added to the quoted fee, in basis points.
const FEE_BUFFER_BPS: u128 = 100;

/// Fraction of the denomination offered as the fee for the gas estimate.
const SEED_FEE_DENOMINATOR: u64 = 10;

/// Arbitrary mocked address a pool's verifier is moved to during gas estimation.
const SHADOW_VERIFIER: Address = address!("0x00000000000000000000000000004b6f68616b75");

/// Extension trait for [`UserOperationRequest`] that pays for an operation with a tornadocash
/// note.
///
/// Tornadocash paymasters use a shielded note to pay for a `UserOp`'s gas. The remainder of the
/// note is withdrawn to the recipient during validation and available during the operation's
/// execution.
pub trait UserOperationPaymasterExt<C>: Sized {
    /// Pays for `account`'s operation's gas out of `withdrawal`'s note.
    ///
    /// Fixes the operation's gas limits and fees, so preparing it again does not re-estimate.
    ///
    /// # Errors
    /// Returns an error if the pool has no paymaster, if the note cannot cover the operation's
    /// gas, or if proving, estimation, or the fee quote fails.
    #[expect(clippy::too_many_arguments)]
    fn with_tornado_paymaster<S, R, B>(
        self,
        account: &S,
        withdrawal: Withdrawal,
        merkle_proof: &MerkleProof,
        circuit_artifacts: &CircuitArtifacts,
        provider: &impl Provider,
        bundler: &B,
        rng: &mut R,
    ) -> impl std::future::Future<Output = Result<Self, PaymasterError>>
    where
        S: SmartAccount<Call = C>,
        R: rand::CryptoRng,
        B: BundlerApi + PimlicoApi + ?Sized;
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

impl<C> UserOperationPaymasterExt<C> for UserOperationRequest<C> {
    #[tracing::instrument(skip_all)]
    async fn with_tornado_paymaster<S, R, B>(
        self,
        account: &S,
        withdrawal: Withdrawal,
        merkle_proof: &MerkleProof,
        circuit_artifacts: &CircuitArtifacts,
        provider: &impl Provider,
        bundler: &B,
        rng: &mut R,
    ) -> Result<Self, PaymasterError>
    where
        S: SmartAccount<Call = C>,
        R: rand::CryptoRng,
        B: BundlerApi + PimlicoApi + ?Sized,
    {
        let pool = &withdrawal.pool;
        let paymaster_address = pool
            .paymaster
            .ok_or_else(|| PaymasterError::PoolMissingPaymaster(pool.clone()))?;

        let (request, fee) = estimate_fee(
            self,
            account,
            &withdrawal,
            paymaster_address,
            merkle_proof,
            provider,
            bundler,
        )
        .await?;

        //? The placeholder costs what a real proof does, so the estimate holds once proven.
        let proven = withdrawal
            .with_payer(Payer {
                address: paymaster_address,
                fee,
                refund: U256::ZERO,
            })
            .prove(merkle_proof, circuit_artifacts, rng)?;

        Ok(request.with_paymaster(paymaster_address, encode_paymaster_data(&proven)))
    }
}

/// Estimates the operation's gas and fixes its gas limits and fees.
async fn estimate_fee<S, B>(
    request: UserOperationRequest<S::Call>,
    account: &S,
    withdrawal: &Withdrawal,
    paymaster_address: Address,
    merkle_proof: &MerkleProof,
    provider: &impl Provider,
    bundler: &B,
) -> Result<(UserOperationRequest<S::Call>, U256), PaymasterError>
where
    S: SmartAccount,
    B: BundlerApi + PimlicoApi + ?Sized,
{
    let pool = &withdrawal.pool;
    let denomination = U256::from(pool.amount_wei);

    //? The paymaster still checks the fee against the bundler's max cost, and
    //? the operation's calls may spend what the fee leaves, so the seed fee must
    //? sit between the two.
    // TODO: Find a better way to handle this. Maybe override the paymaster to accept any fee?
    let seed = denomination / U256::from(SEED_FEE_DENOMINATOR);
    let placeholder = withdrawal
        .clone()
        .with_payer(Payer {
            address: paymaster_address,
            fee: seed,
            refund: U256::ZERO,
        })
        .prove_dummy(merkle_proof);

    let overrides = verifier_override(provider, pool).await?;
    let fees = bundler.get_user_operation_gas_price().await?.slow;
    let estimated = bundler
        .prepare_user_operation(
            account,
            request
                .clone()
                .with_paymaster(paymaster_address, encode_paymaster_data(&placeholder))
                .with_state_override(overrides),
        )
        .await?;
    let request = request
        .with_gas_limits(estimated.user_op.gas_limits())
        .with_max_fee_per_gas(fees.max_fee_per_gas)
        .with_max_priority_fee_per_gas(fees.max_priority_fee_per_gas);

    let cost = U256::from(estimated.total_gas_limit()) * U256::from(fees.max_fee_per_gas);
    let cost = quote_in_fee_token(provider, pool, paymaster_address, cost).await?;
    let fee = cost + cost * U256::from(FEE_BUFFER_BPS) / U256::from(10_000);
    if fee > denomination {
        return Err(PaymasterError::InsufficientDenomination {
            required: fee,
            denomination,
        });
    }

    Ok((request, fee))
}

/// Overrides the pool's verifier contract to accept any proof while consuming similar gas.
///
/// The forwarder's extra call makes a withdrawal about 3k gas more expensive than with a real
/// proof, so estimates on err slightly high.
async fn verifier_override(
    provider: &impl Provider,
    pool: &Pool,
) -> Result<StateOverride, PaymasterError> {
    let verifier = provider.verifier(pool).await?;
    let code = provider.get_code_at(verifier).await?;

    Ok(StateOverridesBuilder::default()
        .with_code(SHADOW_VERIFIER, code)
        .with_code(verifier, forwarder(SHADOW_VERIFIER))
        .build())
}

/// Quotes a gas cost in wei in the pool's fee token.
async fn quote_in_fee_token(
    provider: &impl Provider,
    pool: &Pool,
    paymaster: Address,
    cost: U256,
) -> Result<U256, PaymasterError> {
    let Asset::Erc20 { address: token, .. } = pool.asset else {
        return Ok(cost);
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
        // pop the call's success; mstore(0, 1); return(0, 32)
        &hex!("5afa50600160005260206000f3"),
    ]
    .concat()
    .into()
}
