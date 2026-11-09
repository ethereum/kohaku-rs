#![doc = include_str!("../README.md")]

use alloy::{
    network::TransactionBuilder,
    primitives::{Address, aliases::U192},
    providers::Provider,
    rpc::types::TransactionRequest,
    sol_types::SolCall,
};

use crate::abis::EntryPoint;

mod abis;

/// An account freshness check failure.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The address has code, ETH, or a nonzero transaction nonce.
    #[error("Address {0} has code, an ETH balance, or a nonzero transaction nonce")]
    AddressUsed(Address),
    /// The sender's default `EntryPoint` nonce is nonzero.
    #[error("Sender {sender} has a nonzero default nonce at EntryPoint {entry_point}")]
    SenderUsed {
        /// Account whose nonce is nonzero.
        sender: Address,
        /// `EntryPoint` queried for that nonce.
        entry_point: Address,
    },
    /// The provider could not read the requested state.
    #[error("Provider error: {0}")]
    Provider(#[from] alloy::transports::TransportError),
    /// The `EntryPoint` nonce response could not be decoded.
    #[error("Invalid EntryPoint nonce response: {0}")]
    Abi(#[from] alloy::sol_types::Error),
}

/// Checks that an address has no code, ETH, or transaction nonce.
///
/// This checks current state; ERC-20 balances and historical activity that leaves
/// no current signal are not detected.
///
/// # Errors
/// Returns [`Error::AddressUsed`] when a current-state signal is present, or
/// [`Error::Provider`] when state cannot be read.
pub async fn is_address_unused<P: Provider + ?Sized>(
    provider: &P,
    address: Address,
) -> Result<(), Error> {
    let code = provider.get_code_at(address).await?;
    let balance = provider.get_balance(address).await?;
    let nonce = provider.get_transaction_count(address).await?;

    if !code.is_empty() || !balance.is_zero() || nonce != 0 {
        return Err(Error::AddressUsed(address));
    }

    Ok(())
}

/// Checks that a sender's default `EntryPoint` nonce is zero.
///
/// Reads nonce key zero. Operations using other nonce keys are not detected.
///
/// # Errors
/// Returns [`Error::SenderUsed`] for a nonzero nonce, [`Error::Provider`] when
/// state cannot be read, or [`Error::Abi`] for an invalid nonce response.
pub async fn is_sender_unused<P: Provider + ?Sized>(
    provider: &P,
    entry_point: &Address,
    sender: &Address,
) -> Result<(), Error> {
    let data = EntryPoint::getNonceCall::new((*sender, U192::ZERO)).abi_encode();
    let response = provider
        .call(
            TransactionRequest::default()
                .with_to(*entry_point)
                .with_input(data),
        )
        .await?;
    let nonce = EntryPoint::getNonceCall::abi_decode_returns(&response)?;

    if !nonce.is_zero() {
        return Err(Error::SenderUsed {
            sender: *sender,
            entry_point: *entry_point,
        });
    }

    Ok(())
}
