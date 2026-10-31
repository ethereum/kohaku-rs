use std::sync::Arc;

use alloy::{
    network::Ethereum,
    primitives::U256,
    providers::{DynProvider, Provider, ProviderBuilder},
};
use kohaku_railgun::{
    account::{
        address::RailgunAddress as CoreAddress,
        signer::{PrivateKeySigner, RailgunSigner},
    },
    builder::RailgunBuilder,
    crypto::keys::{ByteKey, SpendingKey, ViewingKey},
    wallet::NoteEntry,
};
use kohaku_railgun_poi::wallet::{PoiWallet, PoiWalletCheckpoint};
use kohaku_railgun_sync::{SubsquidSyncer, default_utxo_syncer};
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::{bigint::JsU64, chain::Chain, hex::Hex};

mod userop;

/// A PPOI-enabled wallet. The caller owns encryption and must persist
/// `checkpointJson()` after each successful sync or proof-processing pass.
#[wasm_bindgen]
pub struct PoiSession {
    wallet: PoiWallet,
    signer: Arc<PrivateKeySigner>,
    address: CoreAddress,
    provider: DynProvider,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsBalance {
    asset: String,
    poi_status: Option<String>,
    amount: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsNote {
    asset: String,
    poi_status: Option<String>,
    amount: String,
    tree_number: u32,
    leaf_index: u32,
    blinded_commitment: String,
    commitment_type: String,
    memo: String,
}

impl From<NoteEntry> for JsNote {
    fn from(note: NoteEntry) -> Self {
        Self {
            asset: note.asset.to_string(),
            poi_status: note.poi_status.map(|s| s.to_string()),
            amount: note.amount.to_string(),
            tree_number: note.tree_number,
            leaf_index: note.leaf_index,
            blinded_commitment: note.blinded_commitment,
            commitment_type: format!("{:?}", note.commitment_type),
            memo: note.memo,
        }
    }
}

fn js_error(error: impl std::fmt::Display) -> JsError {
    JsError::new(&error.to_string())
}

fn json<T: Serialize>(value: &T) -> Result<String, JsError> {
    serde_json::to_string(value).map_err(js_error)
}

#[wasm_bindgen]
impl PoiSession {
    /// Open a known-chain wallet with caller-supplied RPC and encrypted-storage
    /// plaintext. The JSON checkpoint is from `checkpointJson()` or omitted for
    /// a full resync; old combined SDK checkpoints are intentionally unsupported.
    #[wasm_bindgen(js_name = open)]
    pub async fn open(
        chain: &Chain,
        rpc_url: String,
        spending_key: String,
        viewing_key: String,
        checkpoint_json: Option<String>,
        artifact_base_url: Option<String>,
    ) -> Result<PoiSession, JsError> {
        let spending = SpendingKey::from_bytes(Hex(spending_key).fixed::<32>()?);
        let viewing = ViewingKey::from_bytes(Hex(viewing_key).fixed::<32>()?);
        let checkpoint: PoiWalletCheckpoint = checkpoint_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(js_error)?
            .unwrap_or_default();
        let parsed_rpc = reqwest::Url::parse(&rpc_url).map_err(js_error)?;
        if !matches!(parsed_rpc.scheme(), "http" | "https") {
            return Err(JsError::new("Railgun RPC URL must use HTTP or HTTPS"));
        }
        let provider = ProviderBuilder::new()
            .network::<Ethereum>()
            .connect_http(parsed_rpc)
            .erased();
        let actual_chain_id = provider.get_chain_id().await.map_err(js_error)?;
        if actual_chain_id != chain.inner.id {
            return Err(JsError::new(&format!(
                "Railgun RPC chain ID {actual_chain_id} does not match configured chain {}",
                chain.inner.id
            )));
        }

        // The core syncer API requires Arc<dyn UtxoSyncer>. WASM is single-threaded.
        #[allow(clippy::arc_with_non_send_sync)]
        let utxo_syncer = Arc::new(default_utxo_syncer(&chain.inner, provider.clone()));
        let mut builder = RailgunBuilder::new(chain.inner.clone(), provider.clone())
            .with_state(checkpoint.railgun)
            .with_utxo_syncer(utxo_syncer);
        if let Some(base) = artifact_base_url {
            builder = builder.with_artifact_base_url(base);
        }
        let core_wallet = builder.build().await.map_err(js_error)?;
        let txid_syncer = Arc::new(SubsquidSyncer::new(&chain.inner.subsquid_endpoint));
        let mut wallet =
            PoiWallet::new(core_wallet, checkpoint.poi, txid_syncer).map_err(js_error)?;
        let signer = PrivateKeySigner::new_evm(spending, viewing, chain.inner.id);
        let address = signer.address();
        wallet.register(signer.clone()).await.map_err(js_error)?;
        Ok(Self {
            wallet,
            signer,
            address,
            provider,
        })
    }

    #[wasm_bindgen(getter, js_name = address)]
    #[must_use]
    pub fn address(&self) -> String {
        self.address.to_string()
    }

    #[wasm_bindgen(getter, js_name = syncedBlock)]
    #[must_use]
    pub fn synced_block(&self) -> u64 {
        self.wallet.synced_block()
    }

    /// Caller-encrypted checkpoint. Persist it after sync and proof processing.
    #[wasm_bindgen(js_name = checkpointJson)]
    pub fn checkpoint_json(&self) -> Result<String, JsError> {
        json(&self.wallet.state())
    }

    /// Sync UTXOs, TXIDs, and PPOI lists; recover old missing proofs into the
    /// pending queue without sending any proof to the PPOI node.
    #[wasm_bindgen(js_name = syncWithoutPoiSubmission)]
    pub async fn sync_without_poi_submission(&mut self) -> Result<(), JsError> {
        self.wallet
            .sync_without_poi_submission()
            .await
            .map_err(js_error)
    }

    /// Bounded variant for resumable first syncs. Persist `checkpointJson()`
    /// after every successful window.
    #[wasm_bindgen(js_name = syncToWithoutPoiSubmission)]
    pub async fn sync_to_without_poi_submission(&mut self, block: JsU64) -> Result<(), JsError> {
        self.wallet
            .sync_to_without_poi_submission(u64::try_from(block)?)
            .await
            .map_err(js_error)
    }

    /// Generate and send queued proofs. The JSON report describes attempts,
    /// not POI acceptance. Sync lists again before displaying Valid.
    #[wasm_bindgen(js_name = processPendingPoiJson)]
    pub async fn process_pending_poi_json(&mut self) -> Result<String, JsError> {
        json(&self.wallet.process_pending_poi_report().await)
    }

    #[wasm_bindgen(js_name = balancesJson)]
    pub async fn balances_json(&mut self) -> Result<String, JsError> {
        let entries = self.wallet.balance(self.address).await;
        let balances: Vec<_> = entries
            .into_iter()
            .map(|entry| JsBalance {
                asset: entry.asset.to_string(),
                poi_status: entry.poi_status.map(|s| s.to_string()),
                amount: entry.amount.to_string(),
            })
            .collect();
        json(&balances)
    }

    #[wasm_bindgen(js_name = notesJson)]
    pub async fn notes_json(&mut self) -> Result<String, JsError> {
        let entries: Vec<JsNote> = self
            .wallet
            .notes(self.address)
            .await
            .into_iter()
            .map(JsNote::from)
            .collect();
        json(&entries)
    }

    #[wasm_bindgen(js_name = poiPendingJson)]
    pub fn poi_pending_json(&self) -> Result<String, JsError> {
        json(&self.wallet.poi_pending())
    }

    #[wasm_bindgen(js_name = poiListKeysJson)]
    pub fn poi_list_keys_json(&self) -> Result<String, JsError> {
        json(&self.wallet.poi_list_keys())
    }

    #[wasm_bindgen(js_name = poiRecoveredValidJson)]
    pub fn poi_recovered_valid_json(&self) -> Result<String, JsError> {
        json(&self.wallet.poi_recovered_valid())
    }

    #[wasm_bindgen(js_name = poiStatusPerListJson)]
    pub fn poi_status_per_list_json(&self, blinded_commitment: String) -> Result<String, JsError> {
        let commitment = U256::from_be_bytes(Hex(blinded_commitment).fixed::<32>()?);
        let statuses: Vec<_> = self
            .wallet
            .poi_status_per_list(commitment)
            .into_iter()
            .map(|(list, status)| (list, status.to_string()))
            .collect();
        json(&statuses)
    }
}
