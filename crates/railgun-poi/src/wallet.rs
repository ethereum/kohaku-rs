//! Wallet view that explicitly opts into local PPOI observation and proof submission.

use std::{collections::HashMap, sync::Arc};

use alloy::primitives::{Address, U256};
#[cfg(feature = "userop")]
use alloy::providers::Provider;
use kohaku_railgun::{
    RailgunCheckpoint,
    account::{address::RailgunAddress, signer::RailgunSigner},
    caip::AssetId,
    crypto::keys::ViewingPublicKey,
    indexer::{
        IndexedAccountState,
        syncer::{Operation, TxidSyncer},
    },
    note::{Note, utxo::UtxoNote},
    poi::types::PoiStatus,
    transact::{
        AmountError, AmountIntents, Spend, SpendError,
        proved_transaction::{ProvedOperation, ProvedTx},
    },
    wallet::{AnnotatedNote, BalanceEntry, NoteEntry, RailgunError, RailgunWallet},
};
#[cfg(feature = "userop")]
use kohaku_railgun_paymaster::RailgunUserOps;
use rand::CryptoRng;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::provider::{
    PendingPoiSummary, PoiCheckpoint, PoiProvider, PoiProviderError, list_key_string,
};

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct PoiWalletCheckpoint {
    pub railgun: RailgunCheckpoint,
    pub poi: PoiCheckpoint,
}

#[derive(Debug, Error)]
pub enum PoiWalletError {
    #[error("Railgun error: {0}")]
    Railgun(#[from] RailgunError),
    #[error("POI error: {0}")]
    Poi(#[from] PoiProviderError),
    #[error("Spend error: {0}")]
    Spend(#[from] SpendError),
    #[error("Amount error: {0}")]
    Amount(#[from] AmountError),
    #[error("POI status unavailable; refresh observation before building a transaction")]
    PoiStatusUnavailable,
}

pub struct PoiWallet {
    wallet: RailgunWallet,
    poi: PoiProvider,
}

pub struct PoiTransactor<'a> {
    wallet: &'a mut PoiWallet,
}

/// Valid inputs and account/asset pairs whose status has not yet been observed.
struct SpendableNotes {
    notes: Vec<UtxoNote>,
    unavailable: Vec<(ViewingPublicKey, AssetId)>,
}

impl SpendableNotes {
    fn from_unspent(entries: Vec<AnnotatedNote>) -> Self {
        let unavailable = entries
            .iter()
            .filter(|entry| entry.poi_status == Some(PoiStatus::Unavailable))
            .map(|entry| (entry.note.viewing_pubkey, entry.note.asset))
            .collect();
        let notes = entries
            .into_iter()
            .filter(|entry| entry.poi_status == Some(PoiStatus::Valid))
            .map(|entry| entry.note)
            .collect();
        Self { notes, unavailable }
    }

    fn explain(
        &self,
        result: Result<Vec<Spend>, AmountError>,
    ) -> Result<Vec<Spend>, PoiWalletError> {
        match result {
            Err(AmountError::InsufficientBalance { from, asset, .. })
                if self.unavailable.contains(&(from.viewing_pubkey(), asset)) =>
            {
                Err(PoiWalletError::PoiStatusUnavailable)
            }
            result => Ok(result?),
        }
    }
}

impl PoiWallet {
    /// Build a paymaster UserOperation while enforcing the local POI list policy.
    #[cfg(feature = "userop")]
    pub fn user_operations(&mut self, provider: impl Provider + 'static) -> RailgunUserOps<'_> {
        RailgunUserOps::with_policy(&mut self.wallet, provider, &mut self.poi)
    }
    /// Compose a core wallet with a configured POI provider.
    pub fn from_provider(wallet: RailgunWallet, poi: PoiProvider) -> Self {
        Self { wallet, poi }
    }

    /// Opt into PPOI using an explicitly supplied TXID syncer and caller-owned checkpoint.
    pub fn new(
        wallet: RailgunWallet,
        checkpoint: PoiCheckpoint,
        txid_syncer: Arc<dyn TxidSyncer>,
    ) -> Result<Self, PoiWalletError> {
        let chain = wallet.chain();
        let poi = PoiProvider::new(
            chain.id,
            checkpoint,
            txid_syncer,
            chain.poi_endpoint.clone(),
            chain.list_keys.clone(),
        )?;
        Ok(Self { wallet, poi })
    }

    pub fn state(&self) -> PoiWalletCheckpoint {
        PoiWalletCheckpoint {
            railgun: self.wallet.state(),
            poi: self.poi.state(),
        }
    }

    pub fn transactions(&mut self) -> PoiTransactor<'_> {
        PoiTransactor { wallet: self }
    }

    pub fn chain(&self) -> &kohaku_railgun::chain_config::ChainConfig {
        self.wallet.chain()
    }

    pub fn synced_block(&self) -> u64 {
        self.wallet.synced_block()
    }

    pub async fn register(&mut self, signer: Arc<dyn RailgunSigner>) -> Result<(), PoiWalletError> {
        self.wallet.register(signer).await?;
        Ok(())
    }

    pub fn account_state(&self, address: RailgunAddress) -> Option<IndexedAccountState> {
        self.wallet.account_state(address)
    }

    pub fn poi_list_keys(&self) -> Vec<String> {
        self.poi.list_keys().iter().map(list_key_string).collect()
    }

    pub fn own_operations(&self) -> Vec<(String, Operation)> {
        self.poi
            .own_operations()
            .into_iter()
            .map(|(txid, op)| {
                let raw: U256 = txid.into();
                (format!("0x{raw:064x}"), op)
            })
            .collect()
    }

    pub fn poi_status_per_list(&self, commitment: U256) -> Vec<(String, PoiStatus)> {
        self.poi
            .status_per_list(commitment.into())
            .into_iter()
            .map(|(key, status)| (list_key_string(&key), status))
            .collect()
    }

    pub fn poi_pending(&self) -> Vec<PendingPoiSummary> {
        self.poi.pending_summaries()
    }

    pub fn poi_recovered_valid(&self) -> Vec<String> {
        self.poi
            .recovered_valid()
            .into_iter()
            .map(|txid| {
                let raw: U256 = txid.into();
                format!("0x{raw:064x}")
            })
            .collect()
    }

    pub async fn sync(&mut self) -> Result<(), PoiWalletError> {
        self.sync_to(u64::MAX).await
    }

    pub async fn sync_to(&mut self, block: u64) -> Result<(), PoiWalletError> {
        self.sync_to_without_poi_submission(block).await?;
        self.process_pending_poi().await
    }

    pub async fn sync_without_poi_submission(&mut self) -> Result<(), PoiWalletError> {
        self.sync_to_without_poi_submission(u64::MAX).await
    }

    pub async fn sync_to_without_poi_submission(
        &mut self,
        block: u64,
    ) -> Result<(), PoiWalletError> {
        self.wallet.sync_to(block).await?;
        self.poi
            .sync_to(block, &self.wallet.recovery_accounts())
            .await?;
        Ok(())
    }

    /// Explicitly submits the pending proof queue. A successful call does not establish acceptance.
    pub async fn process_pending_poi(&mut self) -> Result<(), PoiWalletError> {
        self.poi.process_pending(self.wallet.prover()).await?;
        Ok(())
    }

    pub async fn discard_pending_poi(
        &mut self,
        operations: &[ProvedOperation],
    ) -> Result<usize, PoiWalletError> {
        Ok(self.poi.discard_ops(operations)?)
    }

    pub fn register_poi_operations(
        &mut self,
        operations: &[ProvedOperation],
    ) -> Result<(), PoiWalletError> {
        self.poi.register_ops(operations)?;
        Ok(())
    }

    pub async fn unspent_notes(&mut self, address: RailgunAddress) -> Vec<AnnotatedNote> {
        self.wallet
            .unspent_notes(address)
            .await
            .into_iter()
            .map(|mut entry| {
                entry.poi_status = Some(self.poi.status(
                    entry.note.blinded_commitment.into(),
                    entry.note.commitment_type,
                ));
                entry
            })
            .collect()
    }

    pub async fn notes(&mut self, address: RailgunAddress) -> Vec<NoteEntry> {
        self.unspent_notes(address)
            .await
            .into_iter()
            .map(|entry| NoteEntry::from_note(entry.note, entry.poi_status))
            .collect()
    }

    pub async fn balance(&mut self, address: RailgunAddress) -> Vec<BalanceEntry> {
        let mut amounts = HashMap::new();
        for entry in self.unspent_notes(address).await {
            *amounts
                .entry((entry.note.asset(), entry.poi_status))
                .or_insert(0) += entry.note.value();
        }
        amounts
            .into_iter()
            .map(|((asset, poi_status), amount)| BalanceEntry {
                asset,
                poi_status,
                amount,
            })
            .collect()
    }

    pub fn shield(&self) -> kohaku_railgun::transact::ShieldBuilder {
        self.wallet.shield()
    }
}

impl PoiTransactor<'_> {
    fn spendable_notes(&self) -> SpendableNotes {
        SpendableNotes::from_unspent(
            self.wallet
                .wallet
                .all_unspent_notes()
                .into_iter()
                .map(|note| {
                    let status = self
                        .wallet
                        .poi
                        .status(note.blinded_commitment.into(), note.commitment_type);
                    AnnotatedNote {
                        note,
                        poi_status: Some(status),
                    }
                })
                .collect(),
        )
    }

    pub async fn resolve_amounts(
        &mut self,
        intents: AmountIntents,
        rng: &mut impl CryptoRng,
    ) -> Result<Vec<Spend>, PoiWalletError> {
        let spendable = self.spendable_notes();
        spendable.explain(intents.resolve(&spendable.notes, rng))
    }

    pub async fn transfer_amount(
        &mut self,
        from: Arc<dyn RailgunSigner>,
        to: RailgunAddress,
        asset: AssetId,
        value: u128,
        memo: &str,
        rng: &mut impl CryptoRng,
    ) -> Result<Vec<Spend>, PoiWalletError> {
        self.resolve_amounts(
            AmountIntents::new().transfer(from, to, asset, value, memo),
            rng,
        )
        .await
    }

    pub async fn unshield_amount(
        &mut self,
        from: Arc<dyn RailgunSigner>,
        to: Address,
        asset: AssetId,
        value: u128,
        rng: &mut impl CryptoRng,
    ) -> Result<Vec<Spend>, PoiWalletError> {
        self.resolve_amounts(AmountIntents::new().unshield(from, to, asset, value)?, rng)
            .await
    }

    fn ensure_notes_valid(&self, spends: &[Spend]) -> Result<(), PoiWalletError> {
        for spend in spends {
            for note in spend.inputs() {
                let status = self
                    .wallet
                    .poi
                    .status(note.blinded_commitment.into(), note.commitment_type);
                if status != PoiStatus::Valid {
                    return Err(PoiWalletError::Spend(SpendError::NoteNotValid {
                        blinded_commitment: note.blinded_commitment,
                        status,
                    }));
                }
            }
        }
        Ok(())
    }

    pub async fn prove_dummy(
        &mut self,
        spends: &[Spend],
        rng: &mut impl CryptoRng,
    ) -> Result<ProvedTx, PoiWalletError> {
        self.ensure_notes_valid(spends)?;
        Ok(self
            .wallet
            .wallet
            .transactions()
            .prove_dummy(spends, rng)
            .await?)
    }

    pub async fn prove(
        &mut self,
        spends: &[Spend],
        rng: &mut impl CryptoRng,
    ) -> Result<ProvedTx, PoiWalletError> {
        self.ensure_notes_valid(spends)?;
        let tx = self.wallet.wallet.transactions().prove(spends, rng).await?;
        self.wallet.poi.register_ops(&tx.proved_operations)?;
        Ok(tx)
    }
}

#[cfg(all(test, native))]
#[path = "wallet_tests.rs"]
mod tests;
