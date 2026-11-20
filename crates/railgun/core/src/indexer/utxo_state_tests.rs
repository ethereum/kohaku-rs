use std::sync::Arc;

use ruint::aliases::U256;

use super::*;
use crate::{
    account::signer::{PrivateKeySigner, RailgunSigner},
    caip::AssetId,
    crypto::keys::{ByteKey, SpendingKey, ViewingKey},
    indexer::syncer::{LegacyCommitment, SyncEvent, SyncerError, UtxoSyncer},
    merkle_tree::MerkleRoot,
    note::encrypt::encrypt_shield,
};

struct OneLeaf;

#[async_trait::async_trait]
impl UtxoSyncer for OneLeaf {
    async fn latest_block(&self) -> Result<u64, SyncerError> {
        Ok(10)
    }
    async fn sync(&self, from: u64, to: u64) -> Result<Vec<SyncEvent>, SyncerError> {
        Ok(if (from..=to).contains(&5) {
            vec![SyncEvent::Legacy(
                LegacyCommitment {
                    hash: U256::from(1),
                    tree_number: 0,
                    leaf_index: 0,
                },
                5,
            )]
        } else {
            vec![]
        })
    }
}

struct Verifier(bool);

#[async_trait::async_trait]
impl MerkleTreeVerifier for Verifier {
    async fn verify_root(
        &self,
        _: u32,
        _: u32,
        _: MerkleRoot,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync + 'static>> {
        Ok(self.0)
    }
}

#[tokio::test]
async fn rejected_root_does_not_publish_state() {
    let mut indexer = UtxoIndexer::new(
        UtxoIndexerState::default(),
        Arc::new(OneLeaf),
        Arc::new(Verifier(false)),
    )
    .unwrap();
    assert!(indexer.sync_to(10).await.is_err());
    assert_eq!(indexer.state().synced_block, 0);
    assert!(indexer.state().trees.is_empty());
}

struct BlockingVerifier(tokio::sync::Notify);

#[async_trait::async_trait]
impl MerkleTreeVerifier for BlockingVerifier {
    async fn verify_root(
        &self,
        _: u32,
        _: u32,
        _: MerkleRoot,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync + 'static>> {
        self.0.notify_one();
        std::future::pending().await
    }
}

#[tokio::test]
async fn cancellation_during_root_validation_keeps_the_caller_checkpoint() {
    let verifier = Arc::new(BlockingVerifier(tokio::sync::Notify::new()));
    let mut indexer = UtxoIndexer::new(
        UtxoIndexerState::default(),
        Arc::new(OneLeaf),
        verifier.clone(),
    )
    .unwrap();
    {
        let future = indexer.sync_to(10);
        tokio::pin!(future);
        tokio::select! {
            result = &mut future => panic!("unexpected completion: {result:?}"),
            () = verifier.0.notified() => {},
        }
    }
    assert_eq!(indexer.state().synced_block, 0);
    assert!(indexer.state().trees.is_empty());
}

#[tokio::test]
async fn caller_checkpoint_restores_tree_and_cursor_together() {
    let mut indexer = UtxoIndexer::new(
        UtxoIndexerState::default(),
        Arc::new(OneLeaf),
        Arc::new(Verifier(true)),
    )
    .unwrap();
    indexer.sync_to(10).await.unwrap();
    let checkpoint = indexer.state();
    assert_eq!(checkpoint.synced_block, 10);
    assert_eq!(checkpoint.trees[&0].leaves.len(), 1);
    let restored =
        UtxoIndexer::new(checkpoint, Arc::new(OneLeaf), Arc::new(Verifier(true))).unwrap();
    assert_eq!(restored.synced_block(), 10);
    assert_eq!(restored.utxo_trees[&0].leaves_len(), 1);
}

struct ShieldSyncer(crate::indexer::syncer::Shield);

#[async_trait::async_trait]
impl UtxoSyncer for ShieldSyncer {
    async fn latest_block(&self) -> Result<u64, SyncerError> {
        Ok(10)
    }
    async fn sync(&self, from: u64, to: u64) -> Result<Vec<SyncEvent>, SyncerError> {
        Ok(if (from..=to).contains(&5) {
            vec![SyncEvent::Shield(self.0.clone(), 5)]
        } else {
            vec![]
        })
    }
}

fn shield_fixture() -> (UtxoIndexer, Arc<dyn RailgunSigner>, Arc<ShieldSyncer>) {
    let signer = PrivateKeySigner::new_evm(
        SpendingKey::from_bytes([1; 32]),
        ViewingKey::from_bytes([2; 32]),
        1,
    );
    let asset = AssetId::erc20(alloy::primitives::address!(
        "1234567890123456789012345678901234567890"
    ));
    let request = encrypt_shield(signer.address(), asset, 100, &mut rand::rng()).unwrap();
    let syncer = Arc::new(ShieldSyncer(crate::indexer::syncer::Shield {
        tree_number: 0,
        leaf_index: 0,
        npk: request.preimage.npk.into(),
        token: asset,
        value: U256::from(100),
        ciphertext: request.ciphertext.clone().into(),
        shield_key: *request.ciphertext.shieldKey,
        hash: None,
    }));
    let mut indexer = UtxoIndexer::new(
        UtxoIndexerState::default(),
        syncer.clone(),
        Arc::new(Verifier(true)),
    )
    .unwrap();
    indexer.register(signer.clone()).unwrap();
    (indexer, signer, syncer)
}

#[tokio::test]
async fn account_notes_round_trip_without_duplicate_on_rescan() {
    let (mut indexer, signer, syncer) = shield_fixture();
    indexer.sync_to(10).await.unwrap();
    assert_eq!(indexer.unspent(signer.address()).len(), 1);
    let state: UtxoIndexerState =
        serde_json::from_slice(&serde_json::to_vec(&indexer.state()).unwrap()).unwrap();
    assert_eq!(state.accounts[&signer.address()].notes.len(), 1);
    let mut restored = UtxoIndexer::new(state, syncer, Arc::new(Verifier(true))).unwrap();
    restored.register(signer.clone()).unwrap();
    let bob = PrivateKeySigner::new_evm(
        SpendingKey::from_bytes([3; 32]),
        ViewingKey::from_bytes([4; 32]),
        1,
    );
    restored.register(bob.clone()).unwrap();
    assert_eq!(restored.synced_block(), 0);
    restored.sync_to(10).await.unwrap();
    assert_eq!(restored.unspent(signer.address()).len(), 1);
    assert!(restored.unspent(bob.address()).is_empty());
}
