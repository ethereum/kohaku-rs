use curve25519_dalek::edwards::CompressedEdwardsY;
use kohaku_railgun::{
    account::{address::RailgunAddress as CoreAddress, chain::ChainId},
    crypto::keys::{ByteKey, MasterPublicKey, SpendingKey, ViewingKey, ViewingPublicKey},
};
use tsify::Ts;
use wasm_bindgen::prelude::*;

use crate::{bigint::JsU64, hex::Hex};

/// A Railgun recipient address. Chain IDs are advisory in the core protocol.
#[wasm_bindgen]
pub struct RailgunAddress {
    pub(crate) inner: CoreAddress,
}

#[wasm_bindgen]
impl RailgunAddress {
    /// Parse and validate a Bech32m Railgun address.
    pub fn parse(text: &str) -> Result<Self, JsError> {
        let inner = text.parse()?;
        validate_viewing_key(&inner)?;
        Ok(Self { inner })
    }

    /// Derive an address from two 32-byte private keys. Keep the inputs secret.
    #[wasm_bindgen(js_name = fromPrivateKeys)]
    pub fn from_private_keys(
        spending_key: &Ts<Hex>,
        viewing_key: &Ts<Hex>,
        chain_id: JsU64,
    ) -> Result<Self, JsError> {
        let spending = SpendingKey::from_bytes(spending_key.to_rust()?.fixed::<32>()?);
        let viewing = ViewingKey::from_bytes(viewing_key.to_rust()?.fixed::<32>()?);
        let chain_id = evm_chain_id(chain_id)?;
        Ok(Self {
            inner: CoreAddress::from_private_keys(spending, viewing, chain_id),
        })
    }

    /// Construct an address from the public master and viewing keys.
    #[wasm_bindgen(js_name = fromPublicKeys)]
    pub fn from_public_keys(
        master_key: &Ts<Hex>,
        viewing_public_key: &Ts<Hex>,
        chain_id: JsU64,
    ) -> Result<Self, JsError> {
        let master = MasterPublicKey::from_bytes(master_key.to_rust()?.fixed::<32>()?);
        let viewing = ViewingPublicKey::from_bytes(viewing_public_key.to_rust()?.fixed::<32>()?);
        let chain_id = evm_chain_id(chain_id)?;
        let inner = CoreAddress::from_public_keys(master, viewing, chain_id);
        validate_viewing_key(&inner)?;
        Ok(Self { inner })
    }

    /// Return the address with an all-chains advisory hint.
    #[wasm_bindgen(js_name = allChains)]
    #[must_use]
    pub fn all_chains(&self) -> Self {
        Self {
            inner: self.inner.with_chain(ChainId::All),
        }
    }

    #[wasm_bindgen(js_name = toString)]
    #[must_use]
    pub fn format(&self) -> String {
        self.inner.to_string()
    }
}

fn evm_chain_id(value: JsU64) -> Result<ChainId, JsError> {
    let value = u64::try_from(value)?;
    if value > 0x00ff_ffff_ffff_ffff {
        return Err(JsError::new(
            "EVM chain ID exceeds Railgun address's 56-bit range",
        ));
    }
    Ok(ChainId::evm(value))
}

fn validate_viewing_key(address: &CoreAddress) -> Result<(), JsError> {
    let bytes = *address.viewing_pubkey().as_bytes();
    if CompressedEdwardsY(bytes).decompress().is_none() {
        return Err(JsError::new(
            "Railgun address has an invalid viewing public key",
        ));
    }
    Ok(())
}

#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_tests {
    use super::*;
    use tsify::Tsify;
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    fn derives_and_parses_the_core_address() {
        let spending = Hex::from([1u8; 32].as_slice()).into_ts().unwrap();
        let viewing = Hex::from([2u8; 32].as_slice()).into_ts().unwrap();
        let address =
            RailgunAddress::from_private_keys(&spending, &viewing, JsU64::from(1u64)).unwrap();
        let text = address.format();
        assert_eq!(RailgunAddress::parse(&text).unwrap().format(), text);
        let master = Hex::from(address.inner.master_key().as_bytes().as_slice())
            .into_ts()
            .unwrap();
        let viewing_public = Hex::from(address.inner.viewing_pubkey().as_bytes().as_slice())
            .into_ts()
            .unwrap();
        assert_eq!(
            RailgunAddress::from_public_keys(&master, &viewing_public, JsU64::from(1u64))
                .unwrap()
                .format(),
            text
        );
        assert!(address.all_chains().format().starts_with("0zk"));
        assert!(
            RailgunAddress::from_private_keys(&spending, &viewing, JsU64::from(1u64 << 56))
                .is_err()
        );
    }

    #[wasm_bindgen_test]
    fn refuses_an_encoded_address_with_an_invalid_viewing_key() {
        let core = CoreAddress::from_public_keys(
            MasterPublicKey::from_bytes([1u8; 32]),
            ViewingPublicKey::from_bytes([2u8; 32]),
            ChainId::evm(1),
        );
        assert!(RailgunAddress::parse(&core.to_string()).is_err());
    }
}
