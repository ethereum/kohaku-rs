use alloy::{
    primitives::{Address, Bytes, U256},
    sol_types::SolValue,
};
use kohaku_userop_kit::{
    builder::UserOperationBuilder,
    bundler::{Bundler, BundlerExt},
};

use crate::{
    merkle_tree::MerkleTree,
    pool::Pool,
    provider::{TornadoProvider, TornadoProviderError},
    userop_provider::abis::{PaymasterData, TornadoAdapterData},
    withdrawal::{ProvenWithdrawal, Withdrawal, WithdrawalError},
};

mod abis;

/// Safety margin added to the quoted fee, in basis points.
const FEE_BUFFER_BPS: u128 = 100;

/// Fraction of the denomination offered as the fee for the first gas estimate.
const SEED_FEE_DENOMINATOR: u64 = 10;

/// Extension trait for [`UserOperationBuilder`] that pays for an operation with a tornadocash
/// note.
///
/// Tornadocash paymasters use a shielded note to pay for a `UserOp`'s gas. The remainder of the
/// note is withdrawn to the recipient during validation and available during the operation's
/// execution.
pub trait UserOperationPaymasterExt: Sized {
    /// Pays for this operation's gas out of `withdrawal`'s note.
    ///
    /// # Errors
    /// Returns an error if the pool has no paymaster, if the note cannot cover the operation's
    /// gas, or if proving, estimation, or the fee quote fails.
    fn with_tornado_paymaster<R>(
        self,
        withdrawal: Withdrawal,
        provider: &TornadoProvider,
        bundler: &dyn Bundler,
        tree: &MerkleTree,
        rng: &mut R,
    ) -> impl std::future::Future<Output = Result<Self, TornadoPaymasterError>>
    where
        R: rand::CryptoRng;
}

#[derive(Debug, thiserror::Error)]
pub enum TornadoPaymasterError {
    #[error("Pool missing paymaster address: {0}")]
    PoolMissingPaymasterAddress(Pool),
    #[error("Pool denomination cannot cover the operation's gas: {required} > {denomination}")]
    InsufficientDenomination { required: U256, denomination: U256 },
    #[error("Operation outgrew the proven fee: {required} > {fee}")]
    InsufficientFee { required: U256, fee: U256 },
    #[error("Bundler error: {0}")]
    Bundler(#[from] kohaku_userop_kit::bundler::BundlerError),
    #[error(transparent)]
    Withdrawal(#[from] WithdrawalError),
    #[error(transparent)]
    TornadoProvider(#[from] TornadoProviderError),
}

impl<S> UserOperationPaymasterExt for UserOperationBuilder<S> {
    #[tracing::instrument(skip_all)]
    async fn with_tornado_paymaster<R>(
        self,
        withdrawal: Withdrawal,
        provider: &TornadoProvider,
        bundler: &dyn Bundler,
        tree: &MerkleTree,
        rng: &mut R,
    ) -> Result<Self, TornadoPaymasterError>
    where
        R: rand::CryptoRng,
    {
        let pool = withdrawal.pool.clone();
        let denomination = U256::from(pool.amount_wei);

        //? The paymaster runs the withdrawal before checking the fee, so even this throwaway
        //? estimate needs a real proof.
        let seed = denomination / U256::from(SEED_FEE_DENOMINATOR);
        let builder = estimate_at_fee(self, withdrawal.clone(), seed, tree, bundler, rng).await?;

        let cost = gas_cost(provider, &pool, &builder).await?;
        let fee = cost + cost * U256::from(FEE_BUFFER_BPS) / U256::from(10_000);
        if fee > denomination {
            return Err(TornadoPaymasterError::InsufficientDenomination {
                required: fee,
                denomination,
            });
        }

        let builder = estimate_at_fee(builder, withdrawal, fee, tree, bundler, rng).await?;

        //? The proof is bound to `fee`, so a grown estimate can only be refused.
        let settled = gas_cost(provider, &pool, &builder).await?;
        if settled > fee {
            return Err(TornadoPaymasterError::InsufficientFee {
                required: settled,
                fee,
            });
        }

        Ok(builder)
    }
}

async fn estimate_at_fee<S, R>(
    builder: UserOperationBuilder<S>,
    withdrawal: Withdrawal,
    fee: U256,
    tree: &MerkleTree,
    bundler: &dyn Bundler,
    rng: &mut R,
) -> Result<UserOperationBuilder<S>, TornadoPaymasterError>
where
    R: rand::CryptoRng,
{
    let (paymaster, adapter) = paymaster_addresses(&withdrawal.pool)?;
    let withdrawal = withdrawal
        .with_relayer_address(paymaster)
        .with_fee(fee)
        .prove(tree, rng)
        .await?;

    Ok(builder
        .with_paymaster_and_data(paymaster, encode_paymaster_data(adapter, &withdrawal))
        .with_gas_estimate(bundler)
        .await?)
}

/// Quotes the operation's maximum gas cost in the pool's fee token.
async fn gas_cost<S>(
    provider: &TornadoProvider,
    pool: &Pool,
    builder: &UserOperationBuilder<S>,
) -> Result<U256, TornadoPaymasterError> {
    let user_op = builder.build();
    let gas = U256::from(user_op.total_gas_limit());
    let price =
        U256::from(user_op.user_op.max_fee_per_gas + user_op.user_op.max_priority_fee_per_gas);

    Ok(provider.quote_wei_in_fee_token(pool, gas * price).await?)
}

fn paymaster_addresses(pool: &Pool) -> Result<(Address, Address), TornadoPaymasterError> {
    let missing = || TornadoPaymasterError::PoolMissingPaymasterAddress(pool.clone());

    let paymaster = pool.paymaster_address.ok_or_else(missing)?;
    let adapter = pool.adapter_address.ok_or_else(missing)?;

    Ok((paymaster, adapter))
}

fn encode_paymaster_data(adapter: Address, withdrawal: &ProvenWithdrawal) -> Bytes {
    let adapter_data = TornadoAdapterData {
        proof: withdrawal.proof_bytes(),
        root: withdrawal.root.into(),
        nullifierHash: withdrawal.note.nullifier_hash().into(),
        recipient: withdrawal.recipient,
        relayer: withdrawal.relayer.unwrap_or_default(),
        fee: withdrawal.fee.unwrap_or_default(),
        refund: withdrawal.refund.unwrap_or_default(),
    };
    let data = PaymasterData {
        adapter,
        adapterData: adapter_data.abi_encode().into(),
    };

    data.abi_encode().into()
}
