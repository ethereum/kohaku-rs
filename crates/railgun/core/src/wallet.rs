use std::{collections::HashMap, sync::Arc};

use alloy::primitives::Address;
use rand::CryptoRng;
use serde::Serialize;
use thiserror::Error;

use crate::{
    account::{address::RailgunAddress, signer::RailgunSigner},
    caip::AssetId,
    chain_config::ChainConfig,
    circuit::groth16_prover::Groth16Prover,
    indexer::utxo_indexer::{UtxoIndexer, UtxoIndexerError},
    note::Note,
    poi::types::{BlindedCommitmentType, PoiStatus},
    transact::{
        AmountError, AmountIntents, ShieldBuilder, Spend, SpendError, prove_operations,
        proved_transaction::{ProvedOperation, ProvedTx},
    },
};

#[derive(Debug, Serialize)]
pub struct BalanceEntry {
    pub asset: AssetId,
    /// If POI is enabled, the spendability status from the local list copy.
    /// `Unavailable` means that list is not caught up. It is never `Missing`.
    /// None when POI is disabled.
    #[serde(rename = "poiStatus")]
    pub poi_status: Option<PoiStatus>,
    pub amount: u128,
}

#[derive(Debug, Serialize)]
pub struct NoteEntry {
    pub asset: AssetId,
    /// If POI is enabled, the spendability status from the local list copy.
    /// `Unavailable` means that list is not caught up. It is never `Missing`.
    /// None when POI is disabled.
    #[serde(rename = "poiStatus")]
    pub poi_status: Option<PoiStatus>,
    pub amount: u128,
    #[serde(rename = "treeNumber")]
    pub tree_number: u32,
    #[serde(rename = "leafIndex")]
    pub leaf_index: u32,
    #[serde(rename = "blindedCommitment")]
    pub blinded_commitment: String,
    #[serde(rename = "commitmentType")]
    pub commitment_type: BlindedCommitmentType,
    pub memo: String,
}

impl NoteEntry {
    pub fn from_note(note: UtxoNote, poi_status: Option<PoiStatus>) -> Self {
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

/// An unspent note plus optional status metadata for extension wallets.
///
/// This is the note the amount helpers select from and that [`Spend::try_new`] takes.
#[derive(Debug, Clone)]
pub struct AnnotatedNote {
    pub note: UtxoNote,
    pub poi_status: Option<PoiStatus>,
}

/// `tx.origin` for which the Railgun verifier skips the SNARK check (`VERIFICATION_BYPASS` in the
/// contracts). Only meaningful as the `from` of an `eth_estimateGas` on a dummy-proof transaction.
pub const VERIFICATION_BYPASS: Address =
    alloy::primitives::address!("0x000000000000000000000000000000000000dEaD");

// Types of the read API, for crates that do not reach into the indexer modules.
pub use crate::{
    indexer::{indexed_account::IndexedAccountState, syncer::Operation},
    note::{
        sent::SentNote,
        utxo::{UtxoNote, blinded_commitment},
    },
    poi::types::BlindedCommitmentType as CommitmentKind,
};

/// Interfaces with the RAILGUN protocol.
pub struct RailgunWallet {
    chain: ChainConfig,
    utxo_indexer: UtxoIndexer,
    prover: Groth16Prover,
}

/// Selects and proves Railgun spends against a wallet's current indexed notes.
pub struct RailgunTransactor<'a> {
    wallet: &'a mut RailgunWallet,
}

#[derive(Debug, Error)]
pub enum RailgunError {
    #[error("UTXO syncer is required; supply one with RailgunBuilder::with_utxo_syncer")]
    MissingUtxoSyncer,
    #[error("Utxo indexer error: {0}")]
    UtxoIndexer(#[from] UtxoIndexerError),
    #[error("Spend error: {0}")]
    Spend(#[from] SpendError),
    #[error("Amount error: {0}")]
    Amount(#[from] AmountError),
    #[error("Signer Error: {0}")]
    Signer(#[from] alloy::signers::Error),
}

impl RailgunWallet {
    pub(crate) fn new(
        chain: ChainConfig,
        utxo_indexer: UtxoIndexer,
        prover: Groth16Prover,
    ) -> Result<Self, RailgunError> {
        Ok(Self {
            chain,
            utxo_indexer,
            prover,
        })
    }

    /// Borrow the current wallet state to select and prove transactions.
    pub fn transactions(&mut self) -> RailgunTransactor<'_> {
        RailgunTransactor { wallet: self }
    }

    /// Chain configuration used by this wallet.
    pub fn chain(&self) -> &ChainConfig {
        &self.chain
    }

    /// Last block the UTXO indexer has been synced to.
    pub fn synced_block(&self) -> u64 {
        self.utxo_indexer.synced_block()
    }

    /// A coherent typed checkpoint that the caller may persist in any format.
    pub fn state(&self) -> crate::RailgunCheckpoint {
        crate::RailgunCheckpoint {
            utxo: self.utxo_indexer.state(),
        }
    }

    /// Full state of a registered account: unspent, spent and sent notes.
    pub fn account_state(&self, address: RailgunAddress) -> Option<IndexedAccountState> {
        self.utxo_indexer.account_state(address)
    }

    /// Register a signer with the wallet. The wallet will index and track
    /// UTXOs for the associated address.
    #[expect(clippy::unused_async, reason = "Preserve the registration async API")]
    pub async fn register(&mut self, signer: Arc<dyn RailgunSigner>) -> Result<(), RailgunError> {
        self.utxo_indexer.register(signer)?;
        Ok(())
    }

    /// Syncs indexed UTXOs to the latest block.
    pub async fn sync(&mut self) -> Result<(), RailgunError> {
        self.sync_to(u64::MAX).await
    }

    /// Syncs indexed UTXOs to a specified block.
    pub async fn sync_to(&mut self, to_block: u64) -> Result<(), RailgunError> {
        self.utxo_indexer.sync_to(to_block).await?;
        Ok(())
    }

    /// Account history for extensions that recover proofs of past operations.
    pub fn recovery_accounts(&self) -> Vec<crate::indexer::recovery::RecoveryAccount> {
        self.utxo_indexer.recovery_accounts()
    }

    /// Unspent notes across all registered accounts for extension builders.
    pub fn all_unspent_notes(&self) -> Vec<UtxoNote> {
        self.utxo_indexer
            .registered()
            .into_iter()
            .flat_map(|address| self.utxo_indexer.unspent(address))
            .collect()
    }

    /// Prover used by this wallet.
    pub fn prover(&self) -> &Groth16Prover {
        &self.prover
    }

    /// Unspent notes for `address`. Core notes have no POI status.
    ///
    /// This is the list [`Spend::try_new`] and the amount helpers read. [`Self::balance`]
    /// is the sum of these notes grouped by asset.
    #[allow(clippy::unused_async)]
    pub async fn unspent_notes(&mut self, address: RailgunAddress) -> Vec<AnnotatedNote> {
        let notes = self.utxo_indexer.unspent(address);
        notes
            .into_iter()
            .map(|note| AnnotatedNote {
                note,
                poi_status: None,
            })
            .collect()
    }

    /// Returns all unspent notes for the given address, as a display record.
    pub async fn notes(&mut self, address: RailgunAddress) -> Vec<NoteEntry> {
        self.unspent_notes(address)
            .await
            .into_iter()
            .map(|annotated| NoteEntry::from_note(annotated.note, annotated.poi_status))
            .collect()
    }

    /// Sum of [`Self::unspent_notes`], grouped by asset.
    pub async fn balance(&mut self, address: RailgunAddress) -> Vec<BalanceEntry> {
        let mut balance_map = HashMap::new();
        for annotated in self.unspent_notes(address).await {
            *balance_map
                .entry((annotated.note.asset(), annotated.poi_status))
                .or_insert(0) += annotated.note.value();
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
}

impl RailgunTransactor<'_> {
    /// Select notes for one transfer and return the spends that cover it.
    ///
    /// A transfer that does not fit on one tree comes back as several spends, one per
    /// tree, each of which is one spending signature at [`Self::prove`].
    pub async fn transfer_amount(
        &mut self,
        from: Arc<dyn RailgunSigner>,
        to: RailgunAddress,
        asset: AssetId,
        value: u128,
        memo: &str,
        rng: &mut impl CryptoRng,
    ) -> Result<Vec<Spend>, RailgunError> {
        self.resolve_amounts(
            AmountIntents::new().transfer(from, to, asset, value, memo),
            rng,
        )
        .await
    }

    /// Select notes for one unshield and return the spends that cover it.
    ///
    /// See [`Self::transfer_amount`] for how a value that spans trees is split.
    pub async fn unshield_amount(
        &mut self,
        from: Arc<dyn RailgunSigner>,
        to: Address,
        asset: AssetId,
        value: u128,
        rng: &mut impl CryptoRng,
    ) -> Result<Vec<Spend>, RailgunError> {
        self.resolve_amounts(AmountIntents::new().unshield(from, to, asset, value)?, rng)
            .await
    }

    /// Resolve several amount intents in one selection.
    ///
    /// Intents for the same signer and asset are fitted together, smallest first, onto
    /// the oldest tree that can hold each one. The result is the note-level spends
    /// [`Self::prove`] accepts.
    pub async fn resolve_amounts(
        &mut self,
        intents: AmountIntents,
        rng: &mut impl CryptoRng,
    ) -> Result<Vec<Spend>, RailgunError> {
        let notes: Vec<_> = self
            .all_unspent()
            .await
            .into_iter()
            .map(|n| n.note)
            .collect();
        Ok(intents.resolve(&notes, rng)?)
    }

    /// Builds the transaction with an all-zero proof, for gas estimation only.
    ///
    /// Call `eth_estimateGas` on the result with `from` set to [`VERIFICATION_BYPASS`]: the
    /// Railgun verifier skips the SNARK check for that origin. Nothing is registered with the
    /// POI extension, no proof is generated, and the spending signer is not asked to sign.
    /// Not usable on the ERC-4337 path, where the bundler simulates with its own origin.
    pub async fn prove_dummy(
        &mut self,
        spends: &[Spend],
        rng: &mut impl CryptoRng,
    ) -> Result<ProvedTx, RailgunError> {
        let operations = self.prove_spends(spends, rng, false).await?;
        Ok(ProvedTx::new(
            self.wallet.chain.railgun_smart_wallet,
            operations,
        ))
    }

    /// Prove spends into one Railgun transaction.
    ///
    /// Each spend is one spending signature.
    pub async fn prove(
        &mut self,
        spends: &[Spend],
        rng: &mut impl CryptoRng,
    ) -> Result<ProvedTx, RailgunError> {
        let operations = self.prove_spends(spends, rng, true).await?;
        Ok(ProvedTx::new(
            self.wallet.chain.railgun_smart_wallet,
            operations,
        ))
    }
}

impl RailgunTransactor<'_> {
    async fn all_unspent(&mut self) -> Vec<AnnotatedNote> {
        let addresses = self.wallet.utxo_indexer.registered();
        let mut all_notes = Vec::new();
        for address in addresses {
            all_notes.extend(self.wallet.unspent_notes(address).await);
        }
        all_notes
    }

    /// Attach the fee, then prove. `real` asks the spending signer; a dummy proof does not.
    /// Proves spends without registering POI work. Extension builders can defer
    /// registration until their surrounding transaction has passed validation.
    pub async fn prove_spends(
        &mut self,
        spends: &[Spend],
        rng: &mut impl CryptoRng,
        real: bool,
    ) -> Result<Vec<ProvedOperation>, RailgunError> {
        let operations = spends
            .iter()
            .cloned()
            .map(Spend::into_operation)
            .collect::<Result<Vec<_>, _>>()?;
        let prover = real.then_some(&self.wallet.prover);
        Ok(prove_operations(
            prover,
            &self.wallet.utxo_indexer.utxo_trees,
            self.wallet.chain.id,
            &operations,
            rng,
        )
        .await?)
    }
}
