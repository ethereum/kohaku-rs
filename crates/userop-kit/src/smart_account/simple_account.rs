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
