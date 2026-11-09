#![cfg(not(target_arch = "wasm32"))]

use alloy::{
    primitives::{Address, B256, U256},
    providers::ProviderBuilder,
    signers::{SignerSync, local::PrivateKeySigner},
    transports::mock::Asserter,
};
use kohaku_userop_kit::{
    builder::UserOperationBuilder,
    signable_user_operation::SignableUserOperation,
    smart_account::{SmartAccount, simple_account::SimpleAccount},
    user_operation::Authorization,
};

fn operation() -> SignableUserOperation {
    let provider = ProviderBuilder::new().connect_mocked_client(Asserter::new());
    let account = SimpleAccount::new(provider, Address::repeat_byte(0x99), 1);
    UserOperationBuilder::<()>::new(account.address(), account.entry_point(), account.domain())
        .with_nonce(U256::from(7))
        .build()
}

fn owner() -> PrivateKeySigner {
    PrivateKeySigner::from_bytes(&B256::repeat_byte(7)).unwrap()
}

#[test]
fn typed_data_hash_matches_user_operation_signing_hash() {
    let operation = operation();

    assert_eq!(
        operation.typed_data().eip712_signing_hash().unwrap(),
        operation.signing_hash()
    );
}

#[test]
fn with_signature_preserves_the_separate_account_sender() {
    let operation = operation();
    let sender = operation.user_op.sender;
    let owner = owner();
    let hash = operation.signing_hash();
    let signature = owner.sign_hash_sync(&hash).unwrap();

    let signed = operation
        .with_signature(owner.address(), signature)
        .unwrap();

    assert_eq!(signed.user_op.sender, sender);
    assert_ne!(signed.user_op.sender, owner.address());
    assert_eq!(signed.user_op.signature.as_ref(), signature.as_bytes());
}

#[test]
fn with_signature_rejects_a_different_owner() {
    let operation = operation();
    let signature = owner().sign_hash_sync(&operation.signing_hash()).unwrap();

    let error = operation
        .with_signature(Address::ZERO, signature)
        .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("does not match the selected owner")
    );
}

#[test]
fn with_signature_rejects_an_unsigned_authorization() {
    let mut operation = operation();
    operation.user_op.authorization = Authorization::Eip7702(alloy::rpc::types::Authorization {
        chain_id: U256::from(1),
        address: Address::repeat_byte(0x77),
        nonce: 9,
    });
    let owner = owner();
    let signature = owner.sign_hash_sync(&operation.signing_hash()).unwrap();

    let error = operation
        .with_signature(owner.address(), signature)
        .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("authorization is still unsigned")
    );
}

#[test]
fn with_signature_preserves_a_signed_authorization() {
    let mut operation = operation();
    let owner = owner();
    let authorization = alloy::rpc::types::Authorization {
        chain_id: U256::from(1),
        address: Address::repeat_byte(0x77),
        nonce: 9,
    };
    let signature = owner
        .sign_hash_sync(&authorization.signature_hash())
        .unwrap();
    operation.user_op.authorization =
        Authorization::SignedEip7702(authorization.into_signed(signature));
    let signature = owner.sign_hash_sync(&operation.signing_hash()).unwrap();

    let signed = operation
        .with_signature(owner.address(), signature)
        .unwrap();

    assert!(matches!(
        signed.user_op.authorization,
        Authorization::SignedEip7702(_)
    ));
}
