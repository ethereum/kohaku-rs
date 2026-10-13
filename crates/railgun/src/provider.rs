use std::{collections::HashMap, sync::Arc};

use alloy::{
    primitives::{Address, B256, Bytes, U256},
    sol_types::SolCall,
};
use kohaku_eip_1193::provider::{Eip1193Error, Eip1193Provider};
use rand::CryptoRng;
use serde::Serialize;
use thiserror::Error;
use tracing::{info, warn};
use kohaku_userop_kit::{
    builder::UserOperationBuilder,
    bundler::{Bundler, BundlerError, BundlerExt},
    signable_user_operation::SignableUserOperation,
    smart_account::SmartAccount,
};

use crate::{
    account::{address::RailgunAddress, chain::ChainId, signer::RailgunSigner},
    adapter_data::{encode_paymaster_data, encode_railgun_adapter_data, paymaster_railgun_address},
    caip::AssetId,
    chain_config::ChainConfig,
    circuit::groth16_prover::Groth16Prover,
    indexer::utxo_indexer::{UtxoIndexer, UtxoIndexerError},
    note::{Note, utxo::UtxoNote},
    poi::{
        provider::{PoiProvider, PoiProviderError},
        types::{BlindedCommitmentType, PoiStatus},
    },
    transact::{
        ShieldBuilder, TransactionBuilder, TransactionBuilderError,
        proved_transaction::{ProvedOperation, ProvedTx},
    },
};

#[derive(Debug, Serialize)]
#[cfg_attr(js, derive(tsify::Tsify))]
pub struct BalanceEntry {
    pub asset: AssetId,
    /// If POI is enabled, the spendability status of the note according to the POI provider.
    /// A failed lookup is `Unavailable`, never `Missing`. None when POI is disabled.
    #[serde(rename = "poiStatus")]
    pub poi_status: Option<PoiStatus>,
    pub amount: u128,
}

#[derive(Debug, Serialize)]
#[cfg_attr(js, derive(tsify::Tsify))]
pub struct NoteEntry {
    pub asset: AssetId,
    /// If POI is enabled, the spendability status of the note according to the POI provider.
    /// A failed lookup is `Unavailable`, never `Missing`. None when POI is disabled.
    #[serde(rename = "poiStatus")]
    pub poi_status: Option<PoiStatus>,
    pub amount: u128,
    #[serde(rename = "treeNumber")]
    pub tree_number: u32,
    #[serde(rename = "leafIndex")]
    pub leaf_index: u32,
    #[serde(rename = "blindedCommitment")]
    #[cfg_attr(js, tsify(type = "`0x${string}`"))]
    pub blinded_commitment: String,
    #[serde(rename = "commitmentType")]
    pub commitment_type: BlindedCommitmentType,
    pub memo: String,
}

impl NoteEntry {
    fn from_note(note: UtxoNote, poi_status: Option<PoiStatus>) -> Self {
        Self {
            asset: note.asset(),
            poi_status,
            amount: note.value(),
            tree_number: note.tree_number,
            leaf_index: note.leaf_index,
            blinded_commitment: format!("0x{:064x}", note.blinded_commitment),
            commitment_type: note.commitment_type,
            memo: note.memo,
        }
    }
}

/// Interfaces with the RAILGUN protocol.
pub struct RailgunProvider {
    chain: ChainConfig,
    provider: Arc<dyn Eip1193Provider>,
    utxo_indexer: UtxoIndexer,
    prover: Groth16Prover,
    poi_provider: Option<PoiProvider>,
}

#[derive(Debug, Error)]
pub enum RailgunProviderError {
    #[error("Utxo indexer error: {0}")]
    UtxoIndexer(#[from] UtxoIndexerError),
    #[error("Build error: {0}")]
    Build(#[from] TransactionBuilderError),
    #[error("POI provider error: {0}")]
    PoiProvider(#[from] PoiProviderError),
    #[error("Unable to construct valid note configuration for fee payment")]
    FeeNoteNotFound,
    #[error("POI status unavailable; refresh observation before building a transaction")]
    PoiStatusUnavailable,
    #[error("Signer Error: {0}")]
    Signer(#[from] alloy::signers::Error),
    #[error("Bundler error: {0}")]
    Bundler(#[from] BundlerError),
    #[error("RPC error: {0}")]
    Rpc(#[from] Eip1193Error),
    #[error("Privacy Paymaster not configured for chain: {0}")]
    PrivacyPaymasterNotConfigured(u64),
    #[error("Other: {0}")]
    Other(Box<dyn std::error::Error + Send + Sync>),
}

impl RailgunProvider {
    pub(crate) async fn new(
        chain: ChainConfig,
        provider: Arc<dyn Eip1193Provider>,
        utxo_indexer: UtxoIndexer,
        prover: Groth16Prover,
        poi_provider: Option<PoiProvider>,
    ) -> Result<Self, RailgunProviderError> {
        Ok(Self {
            chain,
            provider,
            utxo_indexer,
            prover,
            poi_provider,
        })
    }

    /// Register a signer with the provider. The provider will index and track
    /// UTXOs for the associated address.
    pub async fn register(
        &mut self,
        signer: Arc<dyn RailgunSigner>,
    ) -> Result<(), RailgunProviderError> {
        self.utxo_indexer.register(signer).await?;
        Ok(())
    }

    /// Syncs the provider to the latest block.
    pub async fn sync(&mut self) -> Result<(), RailgunProviderError> {
        self.sync_to(u64::MAX).await
    }

    /// Syncs the provider to the specified block.
    pub async fn sync_to(&mut self, to_block: u64) -> Result<(), RailgunProviderError> {
        self.utxo_indexer.sync_to(to_block).await?;

        if let Some(poi_provider) = &mut self.poi_provider {
            // The account history lets the POI provider rebuild proofs for
            // operations it did not build itself.
            let accounts = self.utxo_indexer.recovery_accounts();
            poi_provider
                .sync_to(&self.prover, to_block, &accounts)
                .await?;
        }

        Ok(())
    }

    /// Returns all unspent notes for the given address.
    pub async fn notes(&mut self, address: RailgunAddress) -> Vec<NoteEntry> {
        self.unspent(address)
            .await
            .into_iter()
            .map(|(note, poi_status)| NoteEntry::from_note(note, poi_status))
            .collect()
    }

    /// Returns the balance for the given address.
    ///
    /// If POI is enabled, groups balances by observed POI status. Only `Valid`
    /// is spendable; `Unavailable` means the lookup failed, not that a proof is missing.
    pub async fn balance(&mut self, address: RailgunAddress) -> Vec<BalanceEntry> {
        let mut balance_map = HashMap::new();
        for note in self.notes(address).await {
            *balance_map
                .entry((note.asset, note.poi_status))
                .or_insert(0) += note.amount;
        }

        balance_map
            .into_iter()
            .map(|((asset, poi_status), amount)| BalanceEntry {
                asset,
                poi_status,
                amount,
            })
            .collect()
    }

    /// Helper to create a shield builder.
    pub fn shield(&self) -> ShieldBuilder {
        ShieldBuilder::new(self.chain.clone())
    }

    /// Helper to create a transaction builder.
    pub fn transact(&self) -> TransactionBuilder {
        TransactionBuilder::new()
    }

    /// Forgets the pending POI proofs of operations that never reached the chain, for instance
    /// after the submitter did not answer. Returns how many entries were dropped. Entries of
    /// operations that are never mined are also dropped automatically during sync, once the
    /// grace period has passed.
    pub async fn discard_pending_poi(
        &mut self,
        operations: &[ProvedOperation],
    ) -> Result<usize, RailgunProviderError> {
        match &mut self.poi_provider {
            Some(p) => Ok(p.discard_ops(operations).await?),
            None => Ok(0),
        }
    }

    /// Build a transaction builder into a proved, signable transaction.
    pub async fn build(
        &mut self,
        builder: TransactionBuilder,
        rng: &mut impl CryptoRng,
    ) -> Result<ProvedTx, RailgunProviderError> {
        let operations = self.build_operation(builder, rng).await?;
        if let Some(poi_provider) = &mut self.poi_provider {
            poi_provider.register_ops(&operations).await?;
        }

        let proved_tx = ProvedTx::new(self.chain.railgun_smart_wallet, operations);
        Ok(proved_tx)
    }

    /// Build a transaction builder into a broadcastable 7702 UserOperation.
    ///
    /// Constructs a UserOperation sent from the `delegator_address` that executes the provided
    /// transaction, with an additional fee note transfer to cover the bundler fees. The
    /// `fee_payer` is the signer that will authorize the fee note transfer to the bundler's
    /// address for the estimated fee amount in `fee_token`.
    pub async fn prepare_userop<S: SmartAccount>(
        &mut self,
        builder: TransactionBuilder,
        bundler: &dyn Bundler,
        sender: &S,
        fee_payer: Arc<dyn RailgunSigner>,
        fee_token: Address,
        calldata: S::Call,
        rng: &mut impl CryptoRng,
    ) -> Result<SignableUserOperation, RailgunProviderError> {
        let privacy_paymaster = self.chain.privacy_paymaster.ok_or(
            RailgunProviderError::PrivacyPaymasterNotConfigured(self.chain.id),
        )?;
        let railgun_fee_adapter = self.chain.railgun_fee_adapter.ok_or(
            RailgunProviderError::PrivacyPaymasterNotConfigured(self.chain.id),
        )?;

        if fee_token != self.chain.wrapped_base_token {
            return Err(RailgunProviderError::Other(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Currently only the wrapped base token is supported for fee payment",
            ))));
        }

        let paymaster_railgun_address = paymaster_railgun_address(ChainId::evm(self.chain.id));
        let fee_asset = AssetId::Erc20(fee_token);

        //? Initial arbitrary estimation of fee note value.
        //? IMPORTANT: Needs to be high enough to not cause a revert. Most
        //? bundlers seem to use a fixed maxCost value for estimation (IE 27_000_000
        //? for pimlico). Setting this too low causes an unrecoverable estimation
        //? failure.
        let mut fee_value = 100_000_000;

        let builder = builder.adapt(railgun_fee_adapter, *sender.address().into_word());

        info!("Iteratively building UserOperation to converge on accurate fee estimate");
        for _ in 0..5 {
            let broadcast_builder = builder.clone().transfer(
                fee_payer.clone(),
                paymaster_railgun_address,
                fee_asset,
                fee_value,
                "fee",
            );

            info!(
                "Building broadcast transaction with fee value: {}",
                fee_value
            );
            let operations = self.build_operation(broadcast_builder, rng).await?;

            // Get the fee operation & note so the decrypted commitment data can be sent to the
            // paymaster.
            let fee_operation = get_fee_operation(&operations, fee_asset, fee_value)?;
            let fee_note = get_fee_note(fee_operation, fee_asset, fee_value)?;

            let random = fee_note.random();
            let asset = fee_token;
            let value = fee_value;
            let transactions = operations.iter().map(|op| op.transaction.clone()).collect();

            let paymaster_data = encode_paymaster_data(
                railgun_fee_adapter,
                encode_railgun_adapter_data(random, asset, value, transactions),
            );

            // Construct UserOperation
            let mut signable = UserOperationBuilder::new_with_smart_account(sender)
                .await
                .map_err(|e| RailgunProviderError::Other(Box::new(e)))?
                .with_call(&calldata)
                .with_paymaster_and_data(privacy_paymaster, Bytes::from(paymaster_data))
                .with_gas_estimate(bundler)
                .await?
                .build();

            // Recalculate fee and check for convergence.
            info!("Prepared broadcast transaction: {:?}", signable);
            let estimated_paymaster_verification_gas_limit =
                estimate_paymaster_verification_gas_limit(self.provider.as_ref(), &signable)
                    .await?;
            info!(
                "Estimated paymaster verification gas limit: {}",
                estimated_paymaster_verification_gas_limit
            );
            signable.user_op.paymaster_verification_gas_limit =
                Some(estimated_paymaster_verification_gas_limit);
            let total_gas = signable.total_gas_limit();
            let new_fee = total_gas * signable.user_op.max_fee_per_gas;
            info!("Estimated total gas: {}", total_gas);
            info!(
                "Estimated max fee per gas: {}",
                signable.user_op.max_fee_per_gas
            );

            //? Return once the fee converges within 1% of the previous estimate.
            if new_fee <= fee_value && new_fee.abs_diff(fee_value) <= fee_value / 100 {
                info!("Fee converged at {}, total gas: {}", new_fee, total_gas);
                if let Some(poi_provider) = &mut self.poi_provider {
                    poi_provider.register_ops(&operations).await?;
                }
                return Ok(signable);
            }
            fee_value = new_fee;
            info!("Fee updated to {}", new_fee);
        }

        return Err(RailgunProviderError::Other(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Failed to converge on fee estimate",
        ))));
    }

    async fn all_unspent(&mut self) -> Vec<(UtxoNote, Option<PoiStatus>)> {
        let addresses = self.utxo_indexer.registered();
        let mut all_notes = Vec::new();

        for address in addresses {
            let mut notes = self.unspent(address).await;
            all_notes.append(&mut notes);
        }
        all_notes
    }

    async fn unspent(&mut self, address: RailgunAddress) -> Vec<(UtxoNote, Option<PoiStatus>)> {
        let notes = self.utxo_indexer.unspent(address);

        let Some(poi_provider) = &mut self.poi_provider else {
            return notes.into_iter().map(|note| (note, None)).collect();
        };

        let mut annotated_notes = Vec::new();
        for note in notes {
            let status = poi_provider
                .status(note.blinded_commitment.into(), note.commitment_type)
                .await;
            match status {
                Ok(status) => {
                    annotated_notes.push((note, Some(status)));
                }
                Err(e) => {
                    warn!("Error checking POI for note {}: {}", note, e);
                    annotated_notes.push((note, Some(PoiStatus::Unavailable)));
                }
            }
        }

        annotated_notes
    }

    async fn build_operation(
        &mut self,
        builder: TransactionBuilder,
        rng: &mut impl CryptoRng,
    ) -> Result<Vec<ProvedOperation>, RailgunProviderError> {
        let in_notes = self.all_unspent().await;
        let unavailable: Vec<_> = in_notes
            .iter()
            .filter(|(_, status)| *status == Some(PoiStatus::Unavailable))
            .map(|(note, _)| (note.viewing_pubkey, note.asset))
            .collect();
        let spendable_notes: Vec<UtxoNote> = if let Some(_) = self.poi_provider {
            in_notes
                .into_iter()
                .filter(|(_, status)| *status == Some(PoiStatus::Valid))
                .map(|(note, _)| note)
                .collect()
        } else {
            in_notes.into_iter().map(|(note, _)| note).collect()
        };

        let operations = builder
            .build(
                &self.prover,
                self.chain.id,
                &spendable_notes,
                &self.utxo_indexer.utxo_trees,
                rng,
            )
            .await;

        match operations {
            Err(TransactionBuilderError::InsufficientBalance { from, asset, .. })
                if unavailable.contains(&(from.viewing_pubkey(), asset)) =>
            {
                Err(RailgunProviderError::PoiStatusUnavailable)
            }
            result => result.map_err(Into::into),
        }
    }
}

/// Gets the operation containing the fee note
fn get_fee_operation<'a>(
    operations: &'a Vec<ProvedOperation>,
    fee_asset: AssetId,
    fee_value: u128,
) -> Result<&'a ProvedOperation, RailgunProviderError> {
    let Some(fee_note_pos) = operations.iter().position(|o| {
        o.inner
            .out_notes()
            .iter()
            .any(|n| is_fee_note(n, fee_asset, fee_value))
    }) else {
        return Err(RailgunProviderError::FeeNoteNotFound);
    };
    Ok(&operations[fee_note_pos])
}

/// Gets the fee note from the operation
fn get_fee_note(
    operation: &ProvedOperation,
    fee_asset: AssetId,
    fee_value: u128,
) -> Result<Box<dyn Note>, RailgunProviderError> {
    operation
        .inner
        .out_notes()
        .into_iter()
        .find(|n| is_fee_note(n, fee_asset, fee_value))
        .ok_or(RailgunProviderError::FeeNoteNotFound)
}

fn is_fee_note(note: &Box<dyn Note>, fee_asset: AssetId, fee_value: u128) -> bool {
    note.asset() == fee_asset && note.value() == fee_value && note.memo() == "fee"
}

async fn estimate_paymaster_verification_gas_limit(
    provider: &dyn Eip1193Provider,
    user_op: &SignableUserOperation,
) -> Result<u128, RailgunProviderError> {
    let entry_point = user_op.entry_point;
    let Some(paymaster) = user_op.user_op.paymaster else {
        return Ok(0);
    };
    let user_op = user_op.user_op.into_packed();
    let data = abi::PrivacyPaymaster::validatePaymasterUserOpCall {
        userOp: abi::PrivacyPaymaster::PackedUserOperation {
            sender: user_op.sender,
            nonce: user_op.nonce,
            initCode: user_op.initCode,
            callData: user_op.callData,
            accountGasLimits: user_op.accountGasLimits,
            preVerificationGas: user_op.preVerificationGas,
            gasFees: user_op.gasFees,
            paymasterAndData: user_op.paymasterAndData,
            signature: Bytes::new(),
        },
        userOpHash: B256::ZERO,
        maxCost: U256::ZERO,
    }
    .abi_encode()
    .into();

    Ok(provider
        .estimate_gas(paymaster, data, Some(entry_point))
        .await? as u128)
}

mod abi {
    use alloy::sol;

    sol!(
        contract PrivacyPaymaster {
            function validatePaymasterUserOp(
                PackedUserOperation calldata userOp,
                bytes32 userOpHash,
                uint256 maxCost
            ) external returns (bytes memory context, uint256 validationData);

            struct PackedUserOperation {
                address sender;
                uint256 nonce;
                bytes initCode;
                bytes callData;
                bytes32 accountGasLimits;
                uint256 preVerificationGas;
                bytes32 gasFees;
                bytes paymasterAndData;
                bytes signature;
            }
        }
    );
}

#[cfg(all(test, native))]
mod poi_availability_tests {
    use super::*;
    use crate::{
        account::signer::PrivateKeySigner,
        builder::RailgunBuilder,
        crypto::keys::{ByteKey, SpendingKey, ViewingKey},
        indexer::{
            indexed_account::IndexedAccountState,
            syncer::{SyncEvent, SyncerError, UtxoSyncer},
        },
        railgun_database::RailgunDB,
    };
    use alloy::providers::Provider;
    use kohaku_kv_store::Store;
    use serde_json::{Value, json};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        time::{Duration, timeout},
    };

    const LIST: &str = "public-fixture-list";

    struct NoSync;
    #[async_trait::async_trait]
    impl UtxoSyncer for NoSync {
        async fn latest_block(&self) -> Result<u64, SyncerError> {
            panic!("no sync expected")
        }
        async fn sync(&self, _: u64, _: u64) -> Result<Vec<SyncEvent>, SyncerError> {
            panic!("no sync expected")
        }
    }

    #[derive(Clone)]
    enum Answer {
        Status(PoiStatus),
        RpcError,
        Malformed,
        Incomplete,
        Disconnect,
        Mixed,
    }

    // A bounded loopback endpoint. Public seeded notes only; no chain, indexer,
    // membership or artifact requests are permitted by this fixture.
    async fn service(answer: Answer, calls: usize) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            for call in 0..calls {
                let answer = if matches!(answer, Answer::Mixed) {
                    if call % 2 == 0 {
                        Answer::Status(PoiStatus::Valid)
                    } else {
                        Answer::RpcError
                    }
                } else {
                    answer.clone()
                };
                let (mut socket, _) = timeout(Duration::from_secs(5), listener.accept())
                    .await
                    .unwrap()
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0; 1024];
                let request: Value = loop {
                    let n = timeout(Duration::from_secs(5), socket.read(&mut buffer))
                        .await
                        .unwrap()
                        .unwrap();
                    assert!(n > 0 && bytes.len() + n <= 4096);
                    bytes.extend_from_slice(&buffer[..n]);
                    if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = std::str::from_utf8(&bytes[..end]).unwrap();
                        let len: usize = headers
                            .lines()
                            .find_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                name.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse().unwrap())
                            })
                            .unwrap();
                        if bytes.len() >= end + 4 + len {
                            break serde_json::from_slice(&bytes[end + 4..end + 4 + len]).unwrap();
                        }
                    }
                };
                assert_eq!(request["method"], "ppoi_pois_per_list");
                assert_eq!(request["params"]["chainID"], "11155111");
                if matches!(answer, Answer::Disconnect) {
                    continue;
                }
                let mut body = json!({"jsonrpc": "2.0", "id": request["id"]});
                match &answer {
                    Answer::Status(status) => {
                        let commitment =
                            request["params"]["blindedCommitmentDatas"][0]["blindedCommitment"]
                                .as_str()
                                .unwrap();
                        body["result"] = json!({ commitment: {LIST: status} });
                    }
                    Answer::RpcError => {
                        body["error"] = json!({"code": -32000, "message": "Fixture unavailable"});
                    }
                    Answer::Incomplete => body["result"] = json!({}),
                    Answer::Malformed | Answer::Disconnect | Answer::Mixed => {}
                }
                let body = if matches!(answer, Answer::Malformed) {
                    "not JSON".to_owned()
                } else {
                    body.to_string()
                };
                let wire = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                timeout(Duration::from_secs(5), socket.write_all(wire.as_bytes()))
                    .await
                    .unwrap()
                    .unwrap();
            }
        });
        (url, task)
    }

    async fn fixture(url: &str, poi: bool) -> (RailgunProvider, Arc<dyn RailgunSigner>, AssetId) {
        fixture_with_notes(url, poi, 1).await
    }

    async fn fixture_with_notes(
        url: &str,
        poi: bool,
        count: u32,
    ) -> (RailgunProvider, Arc<dyn RailgunSigner>, AssetId) {
        // Public, never-funded keys. This seeds local state; it is not a scan test.
        let signer = PrivateKeySigner::new_evm(
            SpendingKey::from_bytes([1; 32]),
            ViewingKey::from_bytes([2; 32]),
            11_155_111,
        );
        let asset = AssetId::erc20(Address::repeat_byte(0x11));
        let db = Store::create();
        db.set_account(
            &signer.address(),
            &IndexedAccountState {
                notes: (0..count)
                    .map(|leaf| {
                        UtxoNote::new(
                            0,
                            leaf,
                            signer.clone(),
                            asset,
                            10,
                            [5; 16],
                            "public fixture",
                            BlindedCommitmentType::Transact,
                        )
                    })
                    .collect(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let mut chain = ChainConfig::sepolia();
        chain.poi_endpoint = url.to_owned();
        chain.subsquid_endpoint = url.to_owned();
        chain.list_keys = vec![LIST.into()];
        let rpc = alloy::providers::ProviderBuilder::new()
            .connect_http(url.parse().unwrap())
            .erased();
        let builder = RailgunBuilder::new(chain, rpc)
            .with_database(db)
            .with_utxo_syncer(Arc::new(NoSync));
        // The built-in prover's artifact loader is not injectable yet. A failed
        // status must stop build before that loader can be reached.
        let mut provider = if poi { builder.with_poi() } else { builder }
            .build()
            .await
            .unwrap();
        provider.register(signer.clone()).await.unwrap();
        (provider, signer, asset)
    }

    #[tokio::test]
    async fn failed_lookups_are_unavailable_and_cannot_spend() {
        for answer in [
            Answer::RpcError,
            Answer::Malformed,
            Answer::Incomplete,
            Answer::Disconnect,
        ] {
            let (url, task) = service(answer, 3).await;
            let (mut provider, signer, asset) = fixture(&url, true).await;
            let notes = timeout(Duration::from_secs(5), provider.notes(signer.address()))
                .await
                .unwrap();
            assert_eq!(notes.len(), 1);
            assert_eq!(notes[0].poi_status, Some(PoiStatus::Unavailable));
            assert_eq!(
                serde_json::to_value(&notes[0]).unwrap()["poiStatus"],
                "Unavailable"
            );
            let balance = timeout(Duration::from_secs(5), provider.balance(signer.address()))
                .await
                .unwrap();
            assert_eq!(balance.len(), 1);
            assert_eq!(balance[0].amount, 10);
            assert_eq!(balance[0].poi_status, Some(PoiStatus::Unavailable));
            let builder =
                provider
                    .transact()
                    .transfer(signer.clone(), signer.address(), asset, 1, "");
            let result = timeout(
                Duration::from_secs(5),
                provider.build(builder, &mut rand::rng()),
            )
            .await
            .unwrap();
            assert!(matches!(
                result,
                Err(RailgunProviderError::PoiStatusUnavailable)
            ));
            task.await.unwrap();
        }
    }

    #[tokio::test]
    async fn completed_statuses_keep_their_meaning() {
        for status in [
            PoiStatus::Missing,
            PoiStatus::Valid,
            PoiStatus::ProofSubmitted,
            PoiStatus::ShieldBlocked,
        ] {
            let (url, task) = service(Answer::Status(status), 2).await;
            let (mut provider, signer, _) = fixture(&url, true).await;
            assert_eq!(
                timeout(Duration::from_secs(5), provider.notes(signer.address()))
                    .await
                    .unwrap()[0]
                    .poi_status,
                Some(status)
            );
            assert_eq!(
                timeout(Duration::from_secs(5), provider.balance(signer.address()))
                    .await
                    .unwrap()[0]
                    .poi_status,
                Some(status)
            );
            task.await.unwrap();
        }
    }

    #[tokio::test]
    async fn mixed_notes_remain_visible_in_separate_balance_groups() {
        let (url, task) = service(Answer::Mixed, 6).await;
        let (mut provider, signer, asset) = fixture_with_notes(&url, true, 2).await;
        let notes = timeout(Duration::from_secs(5), provider.notes(signer.address()))
            .await
            .unwrap();
        assert_eq!(
            notes.iter().map(|note| note.poi_status).collect::<Vec<_>>(),
            vec![Some(PoiStatus::Valid), Some(PoiStatus::Unavailable)]
        );
        let balance = timeout(Duration::from_secs(5), provider.balance(signer.address()))
            .await
            .unwrap();
        assert_eq!(balance.len(), 2);
        assert!(
            balance
                .iter()
                .any(|entry| entry.poi_status == Some(PoiStatus::Valid) && entry.amount == 10)
        );
        assert!(balance.iter().any(|entry| entry.poi_status == Some(PoiStatus::Unavailable) && entry.amount == 10));
        // The valid note is sufficient: pass selection despite the unavailable
        // second note, then stop at the absent local tree before proving/artifacts.
        let builder = provider
            .transact()
            .transfer(signer.clone(), signer.address(), asset, 1, "");
        assert!(matches!(
            timeout(
                Duration::from_secs(5),
                provider.build(builder, &mut rand::rng())
            )
            .await
            .unwrap(),
            Err(RailgunProviderError::Build(
                TransactionBuilderError::MissingTree(0)
            ))
        ));
        task.await.unwrap();
    }

    #[tokio::test]
    async fn unavailable_notes_of_another_asset_do_not_mask_insufficient_balance() {
        let (url, task) = service(Answer::RpcError, 1).await;
        let (mut provider, signer, _) = fixture(&url, true).await;
        let builder = provider.transact().transfer(
            signer.clone(),
            signer.address(),
            AssetId::erc20(Address::repeat_byte(0x22)),
            1,
            "",
        );
        assert!(matches!(
            timeout(
                Duration::from_secs(5),
                provider.build(builder, &mut rand::rng())
            )
            .await
            .unwrap(),
            Err(RailgunProviderError::Build(
                TransactionBuilderError::InsufficientBalance { .. }
            ))
        ));
        task.await.unwrap();
    }

    #[tokio::test]
    async fn genuine_missing_is_not_an_availability_error() {
        let (url, task) = service(Answer::Status(PoiStatus::Missing), 1).await;
        let (mut provider, signer, asset) = fixture(&url, true).await;
        let builder = provider
            .transact()
            .transfer(signer.clone(), signer.address(), asset, 1, "");
        let result = timeout(
            Duration::from_secs(5),
            provider.build(builder, &mut rand::rng()),
        )
        .await
        .unwrap();
        assert!(matches!(
            result,
            Err(RailgunProviderError::Build(
                TransactionBuilderError::InsufficientBalance { .. }
            ))
        ));
        task.await.unwrap();
    }

    #[tokio::test]
    async fn disabled_poi_remains_none_without_a_request() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (mut provider, signer, _) = fixture(&url, false).await;
        assert_eq!(
            timeout(Duration::from_secs(5), provider.notes(signer.address()))
                .await
                .unwrap()[0]
                .poi_status,
            None
        );
        assert_eq!(
            timeout(Duration::from_secs(5), provider.balance(signer.address()))
                .await
                .unwrap()[0]
                .poi_status,
            None
        );
        assert!(
            timeout(Duration::from_millis(20), listener.accept())
                .await
                .is_err()
        );
    }

    #[test]
    fn unavailable_round_trips_locally_but_not_as_a_node_status() {
        assert_eq!(PoiStatus::Unavailable.to_string(), "Unavailable");
        assert_eq!(
            serde_json::to_value(PoiStatus::Unavailable).unwrap(),
            "Unavailable"
        );
        assert_eq!(
            serde_json::from_str::<PoiStatus>("\"Unavailable\"").unwrap(),
            PoiStatus::Unavailable
        );
        assert_eq!(
            "Unavailable".parse::<PoiStatus>().unwrap(),
            PoiStatus::Unavailable
        );
        assert!(
            serde_json::from_str::<crate::poi::types::PoiNodeStatus>("\"Unavailable\"").is_err()
        );
    }
}
