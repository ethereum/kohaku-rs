use alloy::{
    network::TransactionBuilder,
    primitives::Address,
    providers::{DynProvider, Provider},
    rpc::types::TransactionRequest,
    sol_types::SolCall,
};
use ruint::aliases::U256;

use crate::{
    abis::tornado::Tornado,
    pool::{Asset, Pool},
};

#[derive(Debug, thiserror::Error)]
pub enum QuoteError {
    #[error("Provider error: {0}")]
    Provider(#[from] alloy::transports::RpcError<alloy::transports::TransportErrorKind>),
    #[error("Sol error: {0}")]
    Sol(#[from] alloy::sol_types::Error),
}

/// Quote the amount of fee token from a given wei amount. If the pool is native, this is a
/// no-op.
///
/// # Errors
/// Returns an error if the quote cannot be queried.
pub async fn quote_wei_in_fee_token(
    provider: &DynProvider,
    pool: &Pool,
    wei_amount: U256,
) -> Result<U256, QuoteError> {
    match pool.asset {
        Asset::Native { .. } => Ok(wei_amount),
        Asset::Erc20 { address, .. } => {
            quote_wei_in_token(provider, pool.address, address, wei_amount).await
        }
    }
}

async fn quote_wei_in_token(
    provider: &DynProvider,
    pool_address: Address,
    token_address: Address,
    wei_amount: U256,
) -> Result<U256, QuoteError> {
    let call = Tornado::quoteWeiInTokenCall::new((token_address, wei_amount)).abi_encode();

    let result = provider
        .call(
            TransactionRequest::default()
                .with_to(pool_address)
                .input(call.into()),
        )
        .await?;

    let result = Tornado::quoteWeiInTokenCall::abi_decode_returns(&result)?;

    Ok(result)
}
