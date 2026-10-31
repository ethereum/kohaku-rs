use std::sync::Arc;

use alloy::{
    primitives::{Address, B256, Bytes, U256},
    signers::local::PrivateKeySigner as AccountSigner,
};
use kohaku_railgun::{account::signer::RailgunSigner, caip::AssetId, transact::Spend};
use kohaku_userop_kit::{
    bundler::pimlico::PimlicoBundler,
    signed_user_operation::SignedUserOperation,
    smart_account::simple_7702_smart_account::{Call as AccountCall, Simple7702SmartAccount},
};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use super::{PoiSession, js_error, json};
use crate::{address::RailgunAddress, bigint::JsU128, hex::Hex};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExecutionCall {
    target: String,
    value: String,
    data: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PreparedUserOp {
    signed: SignedUserOperation,
    user_op_hash: String,
}

fn account_calls(text: &str) -> Result<Vec<AccountCall>, JsError> {
    let calls: Vec<ExecutionCall> = serde_json::from_str(text).map_err(js_error)?;
    calls
        .into_iter()
        .map(|call| {
            Ok(AccountCall {
                target: Address::try_from(Hex(call.target))?,
                value: call.value.parse().map_err(js_error)?,
                data: Bytes::from(Hex(call.data).bytes()?),
            })
        })
        .collect()
}

fn gross_up(amount: u128, fee_bps: u16) -> Result<u128, &'static str> {
    let divisor = 10_000u16
        .checked_sub(fee_bps)
        .filter(|v| *v > 0)
        .ok_or("invalid Railgun unshield fee")?;
    let numerator = U256::from(amount) * U256::from(10_000);
    let divisor = U256::from(divisor);
    let gross = numerator / divisor + U256::from(numerator % divisor != U256::ZERO);
    if gross >= U256::from(1u128 << 120) {
        return Err("unshield amount exceeds Railgun's 120-bit note limit");
    }
    gross
        .try_into()
        .map_err(|_| "unshield amount exceeds Railgun's 120-bit note limit")
}

fn weth_withdraw(weth: Address, amount: u128) -> AccountCall {
    let mut calldata = vec![0x2e, 0x1a, 0x7d, 0x4d];
    calldata.extend_from_slice(&U256::from(amount).to_be_bytes::<32>());
    AccountCall {
        target: weth,
        value: U256::ZERO,
        data: calldata.into(),
    }
}

impl PoiSession {
    async fn sign_userop(
        &mut self,
        spends: &[Spend],
        account_private_key: String,
        bundler_url: String,
        calls: Vec<AccountCall>,
    ) -> Result<String, JsError> {
        let key = B256::from(Hex(account_private_key).fixed::<32>()?);
        let account_signer = AccountSigner::from_bytes(&key).map_err(js_error)?;
        let account = Simple7702SmartAccount::new(
            self.provider.clone(),
            account_signer.address(),
            self.wallet.chain().id,
        );
        let bundler_url = reqwest::Url::parse(&bundler_url).map_err(js_error)?;
        if !matches!(bundler_url.scheme(), "http" | "https") {
            return Err(JsError::new("Railgun bundler URL must use HTTP or HTTPS"));
        }
        let bundler = PimlicoBundler::new(bundler_url);
        let fee_payer: Arc<dyn RailgunSigner> = self.signer.clone();
        let fee_token = self.wallet.chain().wrapped_base_token;
        let signable = self
            .wallet
            .user_operations(self.provider.clone())
            .prepare_userop_single(
                spends,
                &bundler,
                &account,
                fee_payer,
                fee_token,
                calls,
                &mut rand::rng(),
            )
            .await
            .map_err(js_error)?;
        let signed = signable.sign(&account_signer).await.map_err(js_error)?;
        let result = PreparedUserOp {
            user_op_hash: format!("{:#x}", signed.hash(self.wallet.chain().id).0),
            signed,
        };
        json(&result)
    }
}

#[wasm_bindgen]
impl PoiSession {
    /// Prove a private transfer and prepare its signed paymaster `UserOperation`.
    /// Persist `checkpointJson()` before submitting `signed` to the bundler.
    #[wasm_bindgen(js_name = prepareTransferUserOpJson)]
    pub async fn prepare_transfer_userop_json(
        &mut self,
        recipient: &RailgunAddress,
        token: String,
        amount: JsU128,
        memo: String,
        account_private_key: String,
        bundler_url: String,
    ) -> Result<String, JsError> {
        let token = Address::try_from(Hex(token))?;
        let amount = u128::try_from(amount)?;
        if amount >= 1u128 << 120 {
            return Err(JsError::new(
                "transfer amount exceeds Railgun's 120-bit note limit",
            ));
        }
        let signer: Arc<dyn RailgunSigner> = self.signer.clone();
        let spends = self
            .wallet
            .transactions()
            .transfer_amount(
                signer,
                recipient.inner,
                AssetId::Erc20(token),
                amount,
                &memo,
                &mut rand::rng(),
            )
            .await
            .map_err(js_error)?;
        self.sign_userop(&spends, account_private_key, bundler_url, Vec::new())
            .await
    }

    /// Prove an unshield and prepare a signed paymaster `UserOperation`. For
    /// native ETH, unshield WETH and prefix `WETH.withdraw` before user calls.
    /// User calls may only execute on the unshield recipient's own 7702 account.
    /// The amount is the desired recipient amount; protocol fee is added on top.
    #[wasm_bindgen(js_name = prepareUnshieldUserOpJson)]
    #[allow(clippy::too_many_arguments)]
    pub async fn prepare_unshield_userop_json(
        &mut self,
        recipient: String,
        token: String,
        amount: JsU128,
        unwrap_native: bool,
        tail_calls_json: String,
        account_private_key: String,
        bundler_url: String,
    ) -> Result<String, JsError> {
        let recipient = Address::try_from(Hex(recipient))?;
        let token = Address::try_from(Hex(token))?;
        let amount = u128::try_from(amount)?;
        let mut calls = account_calls(&tail_calls_json)?;
        let key = B256::from(Hex(account_private_key.clone()).fixed::<32>()?);
        let account = AccountSigner::from_bytes(&key).map_err(js_error)?;
        if (unwrap_native || !calls.is_empty()) && recipient != account.address() {
            return Err(JsError::new(
                "Railgun native unwrap and tail calls require a wallet-owned recipient account",
            ));
        }
        let weth = self.wallet.chain().wrapped_base_token;
        if unwrap_native {
            if token != weth {
                return Err(JsError::new(
                    "native unshield requires the wrapped base token",
                ));
            }
            calls.insert(0, weth_withdraw(weth, amount));
        }
        let gross_amount = gross_up(amount, self.wallet.chain().unshield_fee_bps)
            .map_err(JsError::new)?;
        let signer: Arc<dyn RailgunSigner> = self.signer.clone();
        let spends = self
            .wallet
            .transactions()
            .unshield_amount(
                signer,
                recipient,
                AssetId::Erc20(token),
                gross_amount,
                &mut rand::rng(),
            )
            .await
            .map_err(js_error)?;
        self.sign_userop(&spends, account_private_key, bundler_url, calls)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gross_up_rounds_up_to_cover_protocol_fee() {
        assert_eq!(gross_up(9_975, 25).unwrap(), 10_000);
        assert_eq!(gross_up(1, 25).unwrap(), 2);
        assert!(gross_up((1u128 << 120) - 1, 25).is_err());
        assert_eq!(gross_up((1u128 << 120) - 1, 0).unwrap(), (1u128 << 120) - 1);
    }

    #[test]
    fn weth_unwrap_is_first_and_uses_exact_requested_amount() {
        let call = weth_withdraw(Address::ZERO, 42);
        assert_eq!(call.target, Address::ZERO);
        assert_eq!(&call.data[..4], &[0x2e, 0x1a, 0x7d, 0x4d]);
        assert_eq!(&call.data[4..], &U256::from(42).to_be_bytes::<32>());
    }
}
