//! eth-infinitism SimpleAccount v0.8, deployed or created by SimpleAccountFactory.
//! Same atomic execution ABI and EntryPoint EIP-712 signature as Simple7702.
use alloy::{
    network::TransactionBuilder,
    primitives::{Address, Bytes, U256, aliases::U192},
    providers::Provider,
    rpc::types::{Authorization, TransactionRequest},
    sol,
};
use alloy_sol_types::{Eip712Domain, SolCall};
use anyhow::Context;

use crate::{
    abis::entry_point::EntryPoint,
    entry_point::{ENTRY_POINT_08, entry_point_08_domain},
    smart_account::{
        SmartAccount, SmartAccountError,
        simple_7702_smart_account::{Call, Simple7702SmartAccount},
    },
};

#[derive(Clone)]
pub struct DeployedSimpleAccount<P: Provider> {
    provider: P,
    address: Address,
    chain_id: u64,
    deployment: Option<(Address, Bytes)>,
    allow_used: bool,
}

impl<P: Provider> DeployedSimpleAccount<P> {
    /// An already deployed SimpleAccount v0.8. Its EOA owner signs the UserOperation.
    /// Explicitly permit a deployed/delegated, funded or previously used sender.
    pub fn allow_used(mut self, allow_used: bool) -> Self {
        self.allow_used = allow_used;
        self
    }

    pub fn new(provider: P, address: Address, chain_id: u64) -> Self {
        Self {
            provider,
            address,
            chain_id,
            deployment: None,
            allow_used: false,
        }
    }

    /// Resolve the CREATE2 address from the upstream factory; deploy on first use.
    /// Use the upstream v0.8 factory on the chosen chain, not an older EIP-191 account.
    pub async fn from_factory(
        provider: P,
        factory: Address,
        owner: Address,
        salt: U256,
        chain_id: u64,
    ) -> Result<Self, SmartAccountError> {
        let data = Factory::getAddressCall::new((owner, salt)).abi_encode();
        let ret = provider
            .call(
                TransactionRequest::default()
                    .with_to(factory)
                    .with_input(data),
            )
            .await?;
        let address = Factory::getAddressCall::abi_decode_returns(&ret)
            .context("Invalid SimpleAccountFactory address response")?;
        let data = Factory::createAccountCall::new((owner, salt))
            .abi_encode()
            .into();
        Ok(Self {
            provider,
            address,
            chain_id,
            deployment: Some((factory, data)),
            allow_used: false,
        })
    }
}

#[cfg_attr(native, async_trait::async_trait)]
#[cfg_attr(wasm, async_trait::async_trait(?Send))]
impl<P: Provider> SmartAccount for DeployedSimpleAccount<P> {
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
    async fn validate_sender(&self) -> Result<(), SmartAccountError> {
        super::validate_unused_sender(
            &self.provider,
            self.address(),
            self.entry_point(),
            self.allow_used,
        )
        .await
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
        EntryPoint::getNonceCall::abi_decode_returns(&ret)
            .context("Invalid EntryPoint nonce response")
            .map_err(Into::into)
    }
    async fn authorization(&self) -> Result<Authorization, SmartAccountError> {
        Err(SmartAccountError::Other(anyhow::anyhow!(
            "A deployed account has no EIP-7702 authorization"
        )))
    }
    async fn optional_authorization(&self) -> Result<Option<Authorization>, SmartAccountError> {
        Ok(None)
    }
    async fn factory(&self) -> Result<Option<(Address, Bytes)>, SmartAccountError> {
        if self.deployment.is_some() && self.provider.get_code_at(self.address).await?.is_empty() {
            Ok(self.deployment.clone())
        } else {
            Ok(None)
        }
    }
    fn dummy_signature(&self) -> Bytes {
        Simple7702SmartAccount::new(&self.provider, self.address, self.chain_id).dummy_signature()
    }
    fn abi_encode_call(call: &Self::Call) -> Bytes {
        Simple7702SmartAccount::<P>::abi_encode_call(call)
    }
}

sol! {
    interface Factory {
        function getAddress(address owner, uint256 salt) external view returns (address);
        function createAccount(address owner, uint256 salt) external returns (address);
    }
}

#[cfg(all(test, native))]
mod tests {
    use alloy::{providers::ProviderBuilder, sol_types::SolValue, transports::mock::Asserter};

    use super::*;
    use crate::{builder::UserOperationBuilder, user_operation::Authorization as OpAuthorization};

    #[tokio::test]
    async fn deployed_account_uses_entrypoint_nonce_and_no_authorization() {
        let mock = Asserter::new();
        let provider = ProviderBuilder::new().connect_mocked_client(mock.clone());
        let account_address = Address::repeat_byte(0x99);
        mock.push_success(&Bytes::from(U256::from(7).abi_encode()));
        let account = DeployedSimpleAccount::new(provider, account_address, 1).allow_used(true);
        let operation = UserOperationBuilder::new_with_smart_account(&account)
            .await
            .unwrap()
            .with_call(&vec![Call {
                target: Address::repeat_byte(0x33),
                value: U256::ZERO,
                data: Bytes::new(),
            }])
            .build();
        assert_eq!(operation.user_op.sender, account_address);
        assert_eq!(operation.user_op.nonce, U256::from(7));
        assert!(matches!(
            operation.user_op.authorization,
            OpAuthorization::None
        ));
        assert!(operation.user_op.factory.is_none());
        let json = serde_json::to_value(&operation.user_op).unwrap();
        assert!(json.get("eip7702Auth").is_none());
        assert_eq!(
            operation.typed_data().eip712_signing_hash().unwrap(),
            operation.signing_hash()
        );
    }

    #[tokio::test]
    async fn factory_deployment_is_omitted_once_code_exists() {
        let mock = Asserter::new();
        let provider = ProviderBuilder::new().connect_mocked_client(mock.clone());
        let address = Address::repeat_byte(0x99);
        let owner = Address::repeat_byte(0x44);
        let factory = Address::repeat_byte(0x88);
        mock.push_success(&Bytes::from(address.abi_encode()));
        let account =
            DeployedSimpleAccount::from_factory(provider, factory, owner, U256::from(3), 1)
                .await
                .unwrap()
                .allow_used(true);
        mock.push_success(&Bytes::from(U256::ZERO.abi_encode()));
        mock.push_success(&Bytes::new());
        let operation = UserOperationBuilder::new_with_smart_account(&account)
            .await
            .unwrap()
            .build();
        assert_eq!(operation.user_op.factory, Some(factory));
        assert_eq!(operation.user_op.sender, address);
        let deployment = Factory::createAccountCall::abi_decode(
            operation.user_op.factory_data.as_ref().unwrap(),
        )
        .unwrap();
        assert_eq!(deployment.owner, owner);
        assert_eq!(deployment.salt, U256::from(3));
        assert!(matches!(
            operation.user_op.authorization,
            OpAuthorization::None
        ));
        mock.push_success(&Bytes::from_static(&[0x60, 0x00]));
        assert!(account.factory().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn reused_delegated_sender_reads_both_current_nonces() {
        let mock = Asserter::new();
        let provider = ProviderBuilder::new().connect_mocked_client(mock.clone());
        mock.push_success(&Bytes::from(U256::from(7).abi_encode()));
        mock.push_success(&"0x9");
        let account =
            Simple7702SmartAccount::new(provider, Address::repeat_byte(0x44), 1).allow_used(true);
        let operation = UserOperationBuilder::new_with_smart_account(&account)
            .await
            .unwrap()
            .build();
        assert_eq!(operation.user_op.nonce, U256::from(7));
        if let OpAuthorization::Eip7702(auth) = operation.user_op.authorization {
            assert_eq!(auth.nonce, 9);
        } else {
            panic!("Missing delegation authorization");
        }
    }

    #[tokio::test]
    async fn custom_delegation_uses_selected_target_and_native_nonce() {
        let mock = Asserter::new();
        let provider = ProviderBuilder::new().connect_mocked_client(mock.clone());
        mock.push_success(&"0x9");
        let target = Address::repeat_byte(0x77);
        let account = Simple7702SmartAccount::new(provider, Address::repeat_byte(0x44), 1)
            .with_implementation(target)
            .unwrap();
        let auth = account.optional_authorization().await.unwrap().unwrap();
        assert_eq!(auth.address, target);
        assert_eq!(auth.nonce, 9);
    }
}

#[cfg(all(test, native))]
mod reuse_tests {
    use alloy::{providers::ProviderBuilder, sol_types::SolValue, transports::mock::Asserter};

    use super::*;
    use crate::builder::UserOperationBuilder;

    #[tokio::test]
    async fn rejects_each_touched_signal_without_opt_in() {
        for signal in 0..4 {
            let mock = Asserter::new();
            let provider = ProviderBuilder::new().connect_mocked_client(mock.clone());
            mock.push_success(&if signal == 0 {
                Bytes::from_static(&[0x60])
            } else {
                Bytes::new()
            });
            mock.push_success(&if signal == 1 {
                U256::from(1)
            } else {
                U256::ZERO
            });
            mock.push_success(&if signal == 2 { "0x1" } else { "0x0" });
            mock.push_success(&Bytes::from(
                if signal == 3 {
                    U256::from(1)
                } else {
                    U256::ZERO
                }
                .abi_encode(),
            ));
            let account = DeployedSimpleAccount::new(provider, Address::repeat_byte(0x99), 1);
            let result = UserOperationBuilder::new_with_smart_account(&account).await;
            assert!(
                result
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("allow_used(true)")
            );
        }
    }
}
