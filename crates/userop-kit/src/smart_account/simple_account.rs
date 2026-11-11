//! SimpleAccount v0.8 accounts deployed directly or through SimpleAccountFactory.

use alloy::{
    network::TransactionBuilder,
    primitives::{Address, Bytes, U256, aliases::U192},
    providers::Provider,
    rpc::types::TransactionRequest,
};
use alloy_sol_types::{Eip712Domain, SolCall};
use anyhow::Context;

use crate::{
    abis::{entry_point::EntryPoint, simple_account_factory::SimpleAccountFactory},
    entry_point::{ENTRY_POINT_08, entry_point_08_domain},
    smart_account::{
        AccountInit, DUMMY_SIGNATURE, SmartAccount, SmartAccountError,
        simple_7702_smart_account::{Call, Simple7702SmartAccount},
    },
};

/// A SimpleAccount implementation for the v0.8 EntryPoint.
#[derive(Debug, Clone)]
pub struct SimpleAccount<P: Provider> {
    provider: P,
    address: Address,
    chain_id: u64,
    deployment: Option<(Address, Bytes)>,
}

impl<P: Provider> SimpleAccount<P> {
    /// Uses an account already deployed at `address`.
    pub fn new(provider: P, address: Address, chain_id: u64) -> Self {
        Self {
            provider,
            address,
            chain_id,
            deployment: None,
        }
    }

    /// Resolves an account address from a SimpleAccount v0.8 factory.
    ///
    /// Includes `createAccount(owner, salt)` during initialization while the
    /// resolved address has no code.
    ///
    /// # Errors
    /// Returns an error if the factory address lookup fails or cannot be decoded.
    pub async fn from_factory(
        provider: P,
        factory: Address,
        owner: Address,
        salt: U256,
        chain_id: u64,
    ) -> Result<Self, SmartAccountError> {
        let data = SimpleAccountFactory::getAddressCall::new((owner, salt)).abi_encode();
        let ret = provider
            .call(
                TransactionRequest::default()
                    .with_to(factory)
                    .with_input(data),
            )
            .await?;
        let address = SimpleAccountFactory::getAddressCall::abi_decode_returns(&ret)
            .context("Invalid SimpleAccountFactory address response")?;
        let data = SimpleAccountFactory::createAccountCall::new((owner, salt))
            .abi_encode()
            .into();

        Ok(Self {
            provider,
            address,
            chain_id,
            deployment: Some((factory, data)),
        })
    }
}

#[cfg_attr(native, async_trait::async_trait)]
#[cfg_attr(wasm, async_trait::async_trait(?Send))]
impl<P: Provider> SmartAccount for SimpleAccount<P> {
    type Call = Vec<Call>;

    fn address(&self) -> Address {
        self.address
    }

    fn entry_point(&self) -> Address {
        ENTRY_POINT_08
    }

    fn domain(&self) -> Eip712Domain {
        entry_point_08_domain(self.chain_id)
    }

    async fn nonce(&self) -> Result<U256, SmartAccountError> {
        let data = EntryPoint::getNonceCall::new((self.address, U192::ZERO)).abi_encode();
        let ret = self
            .provider
            .call(
                TransactionRequest::default()
                    .with_to(ENTRY_POINT_08)
                    .with_input(data),
            )
            .await?;

        Ok(EntryPoint::getNonceCall::abi_decode_returns(&ret)
            .context("Invalid EntryPoint nonce response")?)
    }

    async fn init(&self) -> Result<AccountInit, SmartAccountError> {
        let Some((factory, data)) = &self.deployment else {
            return Ok(AccountInit::None);
        };

        if self.provider.get_code_at(self.address).await?.is_empty() {
            Ok(AccountInit::Factory {
                factory: *factory,
                data: data.clone(),
            })
        } else {
            Ok(AccountInit::None)
        }
    }

    fn dummy_signature(&self) -> Bytes {
        DUMMY_SIGNATURE.clone()
    }

    fn abi_encode_call(call: &Self::Call) -> Bytes {
        Simple7702SmartAccount::<P>::abi_encode_call(call)
    }
}

#[cfg(all(test, native))]
mod tests {
    use alloy::{providers::ProviderBuilder, sol_types::SolValue, transports::mock::Asserter};

    use super::*;
    use crate::{builder::UserOperationBuilder, user_operation::Authorization};

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
}
