#![cfg(not(target_arch = "wasm32"))]

use alloy::{
    primitives::{Address, Bytes, U256},
    providers::ProviderBuilder,
    sol_types::{SolCall, SolValue},
    transports::mock::Asserter,
};
use kohaku_userop_kit::{
    abis::simple_account_factory::SimpleAccountFactory,
    builder::UserOperationBuilder,
    smart_account::{AccountInit, SmartAccount, SmartAccountError, simple_account::SimpleAccount},
    user_operation::Authorization,
};

#[tokio::test]
async fn from_factory_resolves_address_from_factory() {
    let mock = Asserter::new();
    let provider = ProviderBuilder::new().connect_mocked_client(mock.clone());
    let expected = Address::repeat_byte(0x99);
    mock.push_success(&Bytes::from(expected.abi_encode()));

    let account = SimpleAccount::from_factory(
        provider,
        Address::repeat_byte(0x88),
        Address::repeat_byte(0x44),
        U256::from(3),
        1,
    )
    .await
    .unwrap();

    assert_eq!(account.address(), expected);
}

#[tokio::test]
async fn init_returns_factory_deployment_when_no_code() {
    let mock = Asserter::new();
    let provider = ProviderBuilder::new().connect_mocked_client(mock.clone());
    let factory = Address::repeat_byte(0x88);
    let owner = Address::repeat_byte(0x44);
    let salt = U256::from(3);
    mock.push_success(&Bytes::from(Address::repeat_byte(0x99).abi_encode()));
    let account = SimpleAccount::from_factory(provider, factory, owner, salt, 1)
        .await
        .unwrap();
    mock.push_success(&Bytes::new());

    let AccountInit::Factory {
        factory: actual_factory,
        data,
    } = account.init().await.unwrap()
    else {
        panic!("Expected account deployment");
    };
    let call = SimpleAccountFactory::createAccountCall::abi_decode(&data).unwrap();
    assert_eq!(actual_factory, factory);
    assert_eq!((call.owner, call.salt), (owner, salt));
}

#[tokio::test]
async fn init_returns_none_when_code_exists() {
    let mock = Asserter::new();
    let provider = ProviderBuilder::new().connect_mocked_client(mock.clone());
    mock.push_success(&Bytes::from(Address::repeat_byte(0x99).abi_encode()));
    let account = SimpleAccount::from_factory(
        provider,
        Address::repeat_byte(0x88),
        Address::repeat_byte(0x44),
        U256::ZERO,
        1,
    )
    .await
    .unwrap();
    mock.push_success(&Bytes::from_static(&[0x60, 0x00]));

    assert!(matches!(account.init().await.unwrap(), AccountInit::None));
}

#[tokio::test]
async fn existing_account_needs_no_initialization_lookup() {
    let provider = ProviderBuilder::new().connect_mocked_client(Asserter::new());
    let account = SimpleAccount::new(provider, Address::repeat_byte(0x99), 1);

    assert!(matches!(account.init().await.unwrap(), AccountInit::None));
}

#[tokio::test]
async fn builds_user_op_with_entrypoint_nonce() {
    let mock = Asserter::new();
    let provider = ProviderBuilder::new().connect_mocked_client(mock.clone());
    let address = Address::repeat_byte(0x99);
    let account = SimpleAccount::new(provider, address, 1);
    mock.push_success(&Bytes::from(U256::from(7).abi_encode()));

    let operation = UserOperationBuilder::new_with_smart_account(&account)
        .await
        .unwrap()
        .build();

    assert_eq!(operation.user_op.nonce, U256::from(7));
    assert_eq!(operation.user_op.sender, address);
}

#[tokio::test]
async fn builder_serializes_existing_account_without_deployment_or_authorization() {
    let mock = Asserter::new();
    let provider = ProviderBuilder::new().connect_mocked_client(mock.clone());
    let account = SimpleAccount::new(provider, Address::repeat_byte(0x99), 1);
    mock.push_success(&Bytes::from(U256::ZERO.abi_encode()));

    let operation = UserOperationBuilder::new_with_smart_account(&account)
        .await
        .unwrap()
        .build();
    let json = serde_json::to_value(operation.user_op).unwrap();

    assert!(json.get("eip7702Auth").is_none());
    assert!(json.get("factory").is_none());
    assert!(json.get("factoryData").is_none());
}

#[tokio::test]
async fn builder_sets_factory_fields_without_delegation_authorization() {
    let mock = Asserter::new();
    let provider = ProviderBuilder::new().connect_mocked_client(mock.clone());
    let factory = Address::repeat_byte(0x88);
    mock.push_success(&Bytes::from(Address::repeat_byte(0x99).abi_encode()));
    let account =
        SimpleAccount::from_factory(provider, factory, Address::repeat_byte(0x44), U256::ZERO, 1)
            .await
            .unwrap();
    mock.push_success(&Bytes::from(U256::ZERO.abi_encode()));
    mock.push_success(&Bytes::new());

    let operation = UserOperationBuilder::new_with_smart_account(&account)
        .await
        .unwrap()
        .build();

    assert_eq!(operation.user_op.factory, Some(factory));
    assert!(operation.user_op.factory_data.is_some());
    assert!(matches!(
        operation.user_op.authorization,
        Authorization::None
    ));
}

#[tokio::test]
async fn init_propagates_code_lookup_errors() {
    let mock = Asserter::new();
    let provider = ProviderBuilder::new().connect_mocked_client(mock.clone());
    mock.push_success(&Bytes::from(Address::repeat_byte(0x99).abi_encode()));
    let account = SimpleAccount::from_factory(
        provider,
        Address::repeat_byte(0x88),
        Address::repeat_byte(0x44),
        U256::ZERO,
        1,
    )
    .await
    .unwrap();
    mock.push_failure_msg("Code lookup unavailable");

    assert!(matches!(
        account.init().await,
        Err(SmartAccountError::ProviderError(_))
    ));
}

#[tokio::test]
async fn from_factory_rejects_malformed_address_responses() {
    let mock = Asserter::new();
    let provider = ProviderBuilder::new().connect_mocked_client(mock.clone());
    mock.push_success(&Bytes::new());

    let result = SimpleAccount::from_factory(
        provider,
        Address::repeat_byte(0x88),
        Address::repeat_byte(0x44),
        U256::ZERO,
        1,
    )
    .await;

    assert!(matches!(result, Err(SmartAccountError::Other(_))));
}
