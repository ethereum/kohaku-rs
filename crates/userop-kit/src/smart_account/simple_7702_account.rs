//! EIP-7702 accounts using the Simple7702 execution ABI.

use alloy::{
    primitives::{Address, Bytes, U256, address},
    providers::Provider,
    rpc::types::Authorization,
};
use alloy_sol_types::Eip712Domain;

use crate::{
    entry_point::{ENTRY_POINT_08, entry_point_08_domain},
    smart_account::{
        AccountInit, Call, ECDSA_STUB_SIGNATURE, SmartAccount, SmartAccountError,
        shared::{encode_execute_batch, entry_point_nonce},
    },
};

/// The eth-infinitism Simple7702Account v0.8 implementation address.
///
/// This is a deterministic deployment address. It is not deployed on every chain.
pub const SIMPLE_7702_ACCOUNT_IMPLEMENTATION_V08: Address =
    address!("0xe6Cae83BdE06E4c305530e199D7217f42808555B");

/// An EIP-7702 account using the Simple7702 execution ABI.
#[derive(Clone)]
pub struct Simple7702Account<P: Provider> {
    owner: Address,
    chain_id: u64,
    provider: P,
    implementation: Address,
}

impl<P: Provider> Simple7702Account<P> {
    /// Creates an account that delegates `owner` to `implementation`.
    ///
    /// The implementation must support Simple7702Account's v0.8 EIP-712
    /// signature validation and atomic execution ABI.
    pub fn new(provider: P, owner: Address, implementation: Address, chain_id: u64) -> Self {
        Self {
            owner,
            chain_id,
            provider,
            implementation,
        }
    }
}

#[cfg_attr(native, async_trait::async_trait)]
#[cfg_attr(wasm, async_trait::async_trait(?Send))]
impl<P: Provider> SmartAccount for Simple7702Account<P> {
    type Call = Call;

    fn entry_point(&self) -> Address {
        ENTRY_POINT_08
    }

    fn domain(&self) -> Eip712Domain {
        entry_point_08_domain(self.chain_id)
    }

    fn address(&self) -> Address {
        self.owner
    }

    async fn nonce(&self) -> Result<U256, SmartAccountError> {
        entry_point_nonce(&self.provider, ENTRY_POINT_08, self.owner).await
    }

    async fn init(&self) -> Result<AccountInit, SmartAccountError> {
        let nonce = self.provider.get_transaction_count(self.owner).await?;

        Ok(AccountInit::Eip7702(Authorization {
            chain_id: U256::from(self.chain_id),
            address: self.implementation,
            nonce,
        }))
    }

    fn stub_signature(&self) -> Bytes {
        ECDSA_STUB_SIGNATURE.clone()
    }

    fn encode_calls(calls: &[Self::Call]) -> Bytes {
        encode_execute_batch(calls)
    }
}

#[cfg(all(test, native))]
mod tests {
    use alloy::{providers::ProviderBuilder, sol_types::SolValue, transports::mock::Asserter};

    use super::*;
    use crate::{builder::UserOperationBuilder, user_operation::Eip7702Auth};

    #[tokio::test]
    async fn init_uses_selected_implementation_and_current_native_nonce() {
        let mock = Asserter::new();
        let provider = ProviderBuilder::new().connect_mocked_client(mock.clone());
        let implementation = Address::repeat_byte(0x77);
        let account =
            Simple7702Account::new(provider, Address::repeat_byte(0x44), implementation, 1);
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
        let account = Simple7702Account::new(
            provider,
            Address::repeat_byte(0x44),
            SIMPLE_7702_ACCOUNT_IMPLEMENTATION_V08,
            1,
        );
        mock.push_success(&Bytes::from(U256::from(7).abi_encode()));
        mock.push_success(&"0x9");

        let operation = UserOperationBuilder::new_with_smart_account(&account)
            .await
            .unwrap()
            .build();
        let Eip7702Auth::Unsigned(auth) = operation.user_op.authorization else {
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
        let account = Simple7702Account::new(
            provider,
            Address::repeat_byte(0x44),
            SIMPLE_7702_ACCOUNT_IMPLEMENTATION_V08,
            1,
        );
        assert_send(UserOperationBuilder::new_with_smart_account(&account));
    }
}
