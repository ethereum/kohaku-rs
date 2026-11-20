use alloy::primitives::Address;
use kohaku_railgun::{
    account::address::RailgunAddress as CoreAddress, caip::AssetId, chain_config::ChainConfig,
    transact::ShieldBuilder,
};
use tsify::{Ts, Tsify};
use wasm_bindgen::prelude::*;

use crate::{
    address::RailgunAddress,
    bigint::JsU128,
    call::{Call, Calls},
    chain::Chain,
    hex::Hex,
};

/// Prepare one or more Railgun shields without broadcasting or persisting state.
#[wasm_bindgen]
pub struct Shield {
    chain: ChainConfig,
    erc20: Vec<(CoreAddress, Address, u128)>,
    native: Vec<(CoreAddress, u128)>,
    native_total: u128,
}

#[wasm_bindgen]
impl Shield {
    #[wasm_bindgen(constructor)]
    #[must_use]
    pub fn new(chain: &Chain) -> Self {
        Self {
            chain: chain.inner.clone(),
            erc20: Vec::new(),
            native: Vec::new(),
            native_total: 0,
        }
    }

    /// Add an ERC20 shield. Amount is in token base units.
    #[wasm_bindgen(js_name = addErc20)]
    pub fn add_erc20(
        &mut self,
        recipient: &RailgunAddress,
        token: &Ts<Hex>,
        amount: JsU128,
    ) -> Result<(), JsError> {
        let token = Address::try_from(token.to_rust()?)?;
        let amount = u128::try_from(amount)?;
        self.erc20.push((recipient.inner, token, amount));
        Ok(())
    }

    /// Add a native-currency shield. Amount is in wei.
    #[wasm_bindgen(js_name = addNative)]
    pub fn add_native(
        &mut self,
        recipient: &RailgunAddress,
        amount: JsU128,
    ) -> Result<(), JsError> {
        let amount = u128::try_from(amount)?;
        self.native_total = self
            .native_total
            .checked_add(amount)
            .ok_or_else(|| JsError::new("native shield total exceeds u128"))?;
        self.native.push((recipient.inner, amount));
        Ok(())
    }

    /// Build transaction calls. ERC20 shielding also requires caller-managed approval.
    pub fn build(self) -> Result<Calls, JsError> {
        let mut builder = ShieldBuilder::new(self.chain);
        for (recipient, token, amount) in self.erc20 {
            builder = builder.shield(recipient, AssetId::Erc20(token), amount);
        }
        for (recipient, amount) in self.native {
            builder = builder.shield_native(recipient, amount);
        }
        let calls = builder.build(&mut rand::rng())?;
        let result = js_sys::Array::new();
        for call in calls {
            let item: JsValue = Call::try_from(call)?.into_ts()?.into();
            result.push(&item);
        }
        Ok(result.unchecked_into())
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_tests {
    use super::*;
    use js_sys::Reflect;
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    fn native_shield_produces_a_relay_call() {
        let chain = Chain::mainnet();
        let recipient = recipient();
        let mut shield = Shield::new(&chain);
        shield
            .add_native(&recipient, JsU128::from(1_000_000u128))
            .unwrap();
        let calls: js_sys::Array = shield.build().unwrap().unchecked_into();
        assert_eq!(calls.length(), 1);
        let call = calls.get(0);
        let to = Reflect::get(&call, &"to".into())
            .unwrap()
            .as_string()
            .unwrap();
        assert_eq!(to, chain.inner.relay_adapt_contract.to_string());
        assert_eq!(
            Reflect::get(&call, &"value".into()).unwrap(),
            JsValue::from(1_000_000u128)
        );
        assert!(
            Reflect::get(&call, &"data".into())
                .unwrap()
                .as_string()
                .unwrap()
                .starts_with("0x")
        );
    }

    #[wasm_bindgen_test]
    fn native_total_overflow_is_rejected() {
        let chain = Chain::mainnet();
        let recipient = recipient();
        let mut shield = Shield::new(&chain);
        shield
            .add_native(&recipient, JsU128::from(u128::MAX))
            .unwrap();
        assert!(shield.add_native(&recipient, JsU128::from(1u128)).is_err());
    }

    #[wasm_bindgen_test]
    fn erc20_shield_produces_a_wallet_call() {
        let chain = Chain::mainnet();
        let recipient = recipient();
        let token = Hex::from(chain.inner.wrapped_base_token).into_ts().unwrap();
        let mut shield = Shield::new(&chain);
        shield
            .add_erc20(&recipient, &token, JsU128::from(1_000_000u128))
            .unwrap();
        let calls: js_sys::Array = shield.build().unwrap().unchecked_into();
        assert_eq!(calls.length(), 1);
        let call = calls.get(0);
        let to = Reflect::get(&call, &"to".into())
            .unwrap()
            .as_string()
            .unwrap();
        assert_eq!(to, chain.inner.railgun_smart_wallet.to_string());
        assert_eq!(
            Reflect::get(&call, &"value".into()).unwrap(),
            JsValue::from(0u128)
        );
    }

    fn recipient() -> RailgunAddress {
        use kohaku_railgun::{
            account::chain::ChainId,
            crypto::keys::{ByteKey, SpendingKey, ViewingKey},
        };
        RailgunAddress {
            inner: CoreAddress::from_private_keys(
                SpendingKey::from_bytes([1u8; 32]),
                ViewingKey::from_bytes([2u8; 32]),
                ChainId::evm(1),
            ),
        }
    }
}
