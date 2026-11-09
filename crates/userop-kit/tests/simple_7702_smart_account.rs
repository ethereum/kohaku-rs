#![cfg(not(target_arch = "wasm32"))]

use alloy::{
    primitives::{Address, Bytes, U256},
    providers::ProviderBuilder,
    sol_types::SolValue,
    transports::mock::Asserter,
};
use kohaku_userop_kit::{
    builder::UserOperationBuilder,
    smart_account::{AccountInit, SmartAccount, simple_7702_smart_account::Simple7702SmartAccount},
    user_operation::Authorization,
};

#[tokio::test]
async fn init_uses_selected_implementation_and_current_native_nonce() {
    let mock = Asserter::new();
    let provider = ProviderBuilder::new().connect_mocked_client(mock.clone());
    let implementation = Address::repeat_byte(0x77);
    let account = Simple7702SmartAccount::new(provider, Address::repeat_byte(0x44), 1)
        .with_implementation(implementation);
    mock.push_success(&"0x9");

    let AccountInit::Eip7702(auth) = account.init().await.unwrap() else {
        panic!("Expected EIP-7702 delegation");
    };

    assert_eq!(auth.address, implementation);
    assert_eq!(auth.nonce, 9);
}

#[tokio::test]
async fn builder_keeps_entrypoint_and_authorization_nonces_separate() {
    let mock = Asserter::new();
    let provider = ProviderBuilder::new().connect_mocked_client(mock.clone());
    let account = Simple7702SmartAccount::new(provider, Address::repeat_byte(0x44), 1);
    mock.push_success(&Bytes::from(U256::from(7).abi_encode()));
    mock.push_success(&"0x9");

    let operation = UserOperationBuilder::new_with_smart_account(&account)
        .await
        .unwrap()
        .build();
    let Authorization::Eip7702(auth) = operation.user_op.authorization else {
        panic!("Expected EIP-7702 delegation");
    };

    assert_eq!(operation.user_op.nonce, U256::from(7));
    assert_eq!(auth.nonce, 9);
    assert!(operation.user_op.factory.is_none());
}

#[test]
fn builder_future_is_send_on_native() {
    fn assert_send(_: impl Send) {}

    let provider = ProviderBuilder::new().connect_mocked_client(Asserter::new());
    let account = Simple7702SmartAccount::new(provider, Address::repeat_byte(0x44), 1);
    assert_send(UserOperationBuilder::new_with_smart_account(&account));
}
