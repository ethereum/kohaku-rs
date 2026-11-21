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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use alloy::{
        primitives::{Bytes, U256},
        providers::ProviderBuilder,
        sol_types::SolValue,
        transports::mock::Asserter,
    };

    use super::*;

    #[tokio::test]
    async fn accepts_an_address_without_current_state_signals() {
        let mock = Asserter::new();
        let provider = ProviderBuilder::new().connect_mocked_client(mock.clone());
        mock.push_success(&Bytes::new());
        mock.push_success(&U256::ZERO);
        mock.push_success(&"0x0");

        is_address_unused(&provider, Address::repeat_byte(0x44))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn rejects_existing_code() {
        let mock = Asserter::new();
        let provider = ProviderBuilder::new().connect_mocked_client(mock.clone());
        let address = Address::repeat_byte(0x44);
        mock.push_success(&Bytes::from_static(&[0xef, 0x01, 0x00]));
        mock.push_success(&U256::ZERO);
        mock.push_success(&"0x0");

        assert!(
            matches!(is_address_unused(&provider, address).await, Err(Error::AddressUsed(actual)) if actual == address)
        );
    }

    #[tokio::test]
    async fn rejects_an_existing_eth_balance() {
        let mock = Asserter::new();
        let provider = ProviderBuilder::new().connect_mocked_client(mock.clone());
        mock.push_success(&Bytes::new());
        mock.push_success(&U256::from(1));
        mock.push_success(&"0x0");

        assert!(matches!(
            is_address_unused(&provider, Address::repeat_byte(0x44)).await,
            Err(Error::AddressUsed(_))
        ));
    }

    #[tokio::test]
    async fn rejects_a_nonzero_native_nonce() {
        let mock = Asserter::new();
        let provider = ProviderBuilder::new().connect_mocked_client(mock.clone());
        mock.push_success(&Bytes::new());
        mock.push_success(&U256::ZERO);
        mock.push_success(&"0x1");

        assert!(matches!(
            is_address_unused(&provider, Address::repeat_byte(0x44)).await,
            Err(Error::AddressUsed(_))
        ));
    }

    #[tokio::test]
    async fn address_check_propagates_provider_failure() {
        let mock = Asserter::new();
        let provider = ProviderBuilder::new().connect_mocked_client(mock.clone());
        mock.push_failure_msg("State unavailable");

        assert!(matches!(
            is_address_unused(&provider, Address::repeat_byte(0x44)).await,
            Err(Error::Provider(_))
        ));
    }

    #[tokio::test]
    async fn accepts_an_unused_entrypoint_nonce() {
        let mock = Asserter::new();
        let provider = ProviderBuilder::new().connect_mocked_client(mock.clone());
        mock.push_success(&Bytes::from(U256::ZERO.abi_encode()));

        is_sender_unused(
            &provider,
            &Address::repeat_byte(0x08),
            &Address::repeat_byte(0x44),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn rejects_a_used_entrypoint_nonce() {
        let mock = Asserter::new();
        let provider = ProviderBuilder::new().connect_mocked_client(mock.clone());
        let entry_point = Address::repeat_byte(0x08);
        let sender = Address::repeat_byte(0x44);
        mock.push_success(&Bytes::from(U256::from(7).abi_encode()));

        assert!(
            matches!(is_sender_unused(&provider, &entry_point, &sender).await,
            Err(Error::SenderUsed { sender: actual_sender, entry_point: actual_entry_point })
                if actual_sender == sender && actual_entry_point == entry_point)
        );
    }

    #[tokio::test]
    async fn sender_check_rejects_an_invalid_nonce_response() {
        let mock = Asserter::new();
        let provider = ProviderBuilder::new().connect_mocked_client(mock.clone());
        mock.push_success(&Bytes::new());

        assert!(matches!(
            is_sender_unused(
                &provider,
                &Address::repeat_byte(0x08),
                &Address::repeat_byte(0x44)
            )
            .await,
            Err(Error::Abi(_))
        ));
    }

    #[tokio::test]
    async fn sender_check_propagates_provider_failure() {
        let mock = Asserter::new();
        let provider = ProviderBuilder::new().connect_mocked_client(mock.clone());
        mock.push_failure_msg("EntryPoint unavailable");

        assert!(matches!(
            is_sender_unused(
                &provider,
                &Address::repeat_byte(0x08),
                &Address::repeat_byte(0x44)
            )
            .await,
            Err(Error::Provider(_))
        ));
    }
}
