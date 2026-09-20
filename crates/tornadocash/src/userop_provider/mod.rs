use alloy::{
    primitives::{Address, Bytes, U256},
    sol,
    sol_types::SolValue,
};
use kohaku_userop_kit::{
    builder::UserOperationBuilder,
    bundler::{Bundler, BundlerExt},
};

use crate::{
    abis::tornado::Tornado, pool::Pool, provider::TornadoProviderError, withdrawal::Withdrawal,
};

const FEE_BUFFER_BPS: u128 = 100; // 1% buffer

/// Extension trait for [`Withdrawal`] that adds support for tornadocash paymasters.
///
/// Tornadocash paymasters use a shielded note to pay for a `UserOp`'s gas. The leftover note value
/// is sent to the withdrawal's recipient address.
pub trait WithdrawalPaymasterExt: Sized {
    fn sponsor<S, R>(
        self,
        bundler: &dyn Bundler,
        builder: UserOperationBuilder<S>,
        rng: &mut R,
    ) -> impl std::future::Future<Output = Result<UserOperationBuilder<S>, TornadoPaymasterError>>
    where
        S: Send + Sync,
        R: rand::CryptoRng;
}

#[derive(Debug, thiserror::Error)]
pub enum TornadoPaymasterError {
    #[error("Pool missing paymaster address: {0}")]
    PoolMissingPaymasterAddress(Pool),
    #[error("Bundler error: {0}")]
    Bundler(#[from] kohaku_userop_kit::bundler::BundlerError),
    #[error(transparent)]
    TornadoProvider(#[from] TornadoProviderError),
}

sol!(
    struct PaymasterData {
        address adapter;
        bytes adapterData;
    }

    struct TornadoAdapterData {
        bytes proof;
        bytes32 root;
        bytes32 nullifierHash;
        address recipient;
        address relayer;
        uint256 fee;
        uint256 refund;
    }
);

impl WithdrawalPaymasterExt for Withdrawal {
    #[tracing::instrument(skip_all)]
    async fn sponsor<S, R>(
        self,
        bundler: &dyn Bundler,
        mut builder: UserOperationBuilder<S>,
        rng: &mut R,
    ) -> Result<UserOperationBuilder<S>, TornadoPaymasterError>
    where
        S: Send + Sync,
        R: rand::CryptoRng,
    {
        let pool = self.pool();

        let paymaster = pool
            .paymaster_address
            .ok_or(TornadoPaymasterError::PoolMissingPaymasterAddress(pool))?;
        let adapter = pool
            .adapter_address
            .ok_or(TornadoPaymasterError::PoolMissingPaymasterAddress(pool))?;

        let mut fee_estimate = U256::from(pool.amount_wei);

        loop {
            let withdraw_call = self
                .clone()
                .with_relayer(paymaster)
                .with_fee(fee_estimate)
                .into_call(rng)
                .await?;
            let paymaster_data = encode_paymaster_data(adapter, withdraw_call);
            builder = builder.with_paymaster_and_data(paymaster, paymaster_data);
            builder = builder.with_gas_estimate(bundler).await?;

            let wei = max_gas(&builder);
            let new_fee_estimate = self.provider().quote_wei_in_fee_token(pool, wei).await?;
            if new_fee_estimate <= fee_estimate {
                break;
            }

            let fee_buffer = new_fee_estimate * U256::from(FEE_BUFFER_BPS) / U256::from(10_000);
            fee_estimate = new_fee_estimate + fee_buffer;
        }

        Ok(builder)
    }
}

/// Encodes the paymaster data for a tornadocash withdrawal call.
fn encode_paymaster_data(adapter: Address, withdraw_call: Tornado::withdrawCall) -> Bytes {
    let adapter_data = TornadoAdapterData {
        proof: withdraw_call._proof,
        root: withdraw_call._root,
        nullifierHash: withdraw_call._nullifierHash,
        recipient: withdraw_call._recipient,
        relayer: withdraw_call._relayer,
        fee: withdraw_call._fee,
        refund: withdraw_call._refund,
    };
    let data = PaymasterData {
        adapter,
        adapterData: adapter_data.abi_encode().into(),
    };
    data.abi_encode().into()
}

/// Returns the maximum gas cost of a user operation builder.
fn max_gas<S>(builder: &UserOperationBuilder<S>) -> U256 {
    let user_op = builder.build();
    let total_gas_limit = U256::from(user_op.total_gas_limit());
    let max_fee_per_gas =
        U256::from(user_op.user_op.max_fee_per_gas + user_op.user_op.max_priority_fee_per_gas);

    total_gas_limit * max_fee_per_gas
}
