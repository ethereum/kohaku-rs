#![cfg(not(target_arch = "wasm32"))]

use alloy::{
    primitives::{Address, Bytes, U256},
    providers::ProviderBuilder,
    sol_types::SolValue,
    transports::mock::Asserter,
};
use kohaku_account_checks::{Error, is_address_unused, is_sender_unused};

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
async fn rejects_existing_code_or_delegation() {
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
