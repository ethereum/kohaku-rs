use crate::{
    signable_user_operation::SignableUserOperation,
    smart_account::{AccountInit, SmartAccount, SmartAccountError},
    user_operation::{Eip7702Auth, UserOperation},
    user_operation_request::UserOperationRequest,
};

/// Fills `request` from `account`, leaving unset gas limits at zero.
pub async fn fill_user_operation<S: SmartAccount + ?Sized>(
    account: &S,
    request: UserOperationRequest<S::Call>,
) -> Result<SignableUserOperation, SmartAccountError> {
    let nonce = account.nonce().await?;
    let (factory, factory_data, authorization) = match account.init().await? {
        AccountInit::None => (None, None, Eip7702Auth::None),
        AccountInit::Eip7702(auth) => (None, None, Eip7702Auth::Unsigned(auth)),
        AccountInit::Factory { factory, data } => (Some(factory), Some(data), Eip7702Auth::None),
    };
    let (paymaster, paymaster_data) = request.paymaster.unzip();

    let mut user_op = UserOperation {
        sender: account.address(),
        nonce,
        factory,
        factory_data,
        call_data: S::encode_calls(&request.calls),
        max_fee_per_gas: request.max_fee_per_gas,
        max_priority_fee_per_gas: request.max_priority_fee_per_gas,
        paymaster,
        paymaster_data,
        authorization,
        signature: request
            .stub_signature
            .unwrap_or_else(|| account.stub_signature()),
        ..UserOperation::default()
    };
    if let Some(gas_limits) = request.gas_limits {
        user_op.set_gas_limits(gas_limits);
    }

    Ok(SignableUserOperation {
        user_op,
        entry_point: account.entry_point(),
        domain: account.domain(),
    })
}

#[cfg(all(test, native))]
mod tests {
    use alloy::{
        primitives::{Address, Bytes, U256},
        providers::ProviderBuilder,
        sol_types::SolValue,
        transports::mock::Asserter,
    };

    use super::*;
    use crate::smart_account::{
        SIMPLE_7702_ACCOUNT_IMPLEMENTATION_V08, Simple7702Account, SimpleAccount,
    };

    #[tokio::test]
    async fn fills_entry_point_nonce() {
        let mock = Asserter::new();
        let provider = ProviderBuilder::new().connect_mocked_client(mock.clone());
        let address = Address::repeat_byte(0x99);
        let account = SimpleAccount::new(provider, address, 1);
        mock.push_success(&Bytes::from(U256::from(7).abi_encode()));

        let operation = fill_user_operation(&account, UserOperationRequest::default())
            .await
            .unwrap();

        assert_eq!(operation.user_op.nonce, U256::from(7));
        assert_eq!(operation.user_op.sender, address);
    }

    #[tokio::test]
    async fn serializes_existing_account_without_deployment_or_authorization() {
        let mock = Asserter::new();
        let provider = ProviderBuilder::new().connect_mocked_client(mock.clone());
        let account = SimpleAccount::new(provider, Address::repeat_byte(0x99), 1);
        mock.push_success(&Bytes::from(U256::ZERO.abi_encode()));

        let operation = fill_user_operation(&account, UserOperationRequest::default())
            .await
            .unwrap();
        let json = serde_json::to_value(operation.user_op).unwrap();

        assert!(json.get("eip7702Auth").is_none());
        assert!(json.get("factory").is_none());
        assert!(json.get("factoryData").is_none());
    }

    #[tokio::test]
    async fn sets_factory_fields_without_delegation_authorization() {
        let mock = Asserter::new();
        let provider = ProviderBuilder::new().connect_mocked_client(mock.clone());
        let factory = Address::repeat_byte(0x88);
        mock.push_success(&Bytes::from(Address::repeat_byte(0x99).abi_encode()));
        let account = SimpleAccount::from_factory(
            provider,
            factory,
            Address::repeat_byte(0x44),
            U256::ZERO,
            1,
        )
        .await
        .unwrap();
        mock.push_success(&Bytes::from(U256::ZERO.abi_encode()));
        mock.push_success(&Bytes::new());

        let operation = fill_user_operation(&account, UserOperationRequest::default())
            .await
            .unwrap();

        assert_eq!(operation.user_op.factory, Some(factory));
        assert!(operation.user_op.factory_data.is_some());
        assert!(matches!(operation.user_op.authorization, Eip7702Auth::None));
    }

    #[tokio::test]
    async fn keeps_entrypoint_and_authorization_nonces_separate() {
        let mock = Asserter::new();
        let provider = ProviderBuilder::new().connect_mocked_client(mock.clone());
        let account = Simple7702Account::new(
            provider,
            Address::repeat_byte(0x44),
            SIMPLE_7702_ACCOUNT_IMPLEMENTATION_V08,
            1,
        );
        mock.push_success(&Bytes::from(U256::from(7).abi_encode()));
        mock.push_success(&"0x9");

        let operation = fill_user_operation(&account, UserOperationRequest::default())
            .await
            .unwrap();
        let Eip7702Auth::Unsigned(auth) = operation.user_op.authorization else {
            panic!("Expected EIP-7702 delegation");
        };

        assert_eq!(operation.user_op.nonce, U256::from(7));
        assert_eq!(auth.nonce, 9);
        assert!(operation.user_op.factory.is_none());
    }
}
