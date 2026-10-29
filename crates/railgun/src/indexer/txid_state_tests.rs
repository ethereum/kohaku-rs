use std::sync::{Arc, Mutex};

use super::*;
use crate::{
    merkle_tree::MerkleRoot,
    poi::types::{ListKey, TransactProofData, ValidatedRailgunTxidStatus},
};

struct Syncer;

#[async_trait::async_trait]
impl TxidSyncer for Syncer {
    async fn latest_block(&self) -> Result<u64, SyncerError> {
        Ok(10)
    }
    async fn sync(&self, from: u64, to: u64) -> Result<Vec<Operation>, SyncerError> {
        Ok(if (from..=to).contains(&10) {
            vec![Operation {
                block_number: 10,
                nullifiers: vec![U256::from(1)],
                commitment_hashes: vec![U256::from(2)],
                bound_params_hash: U256::from(3),
                utxo_tree_in: 0,
                utxo_tree_out: 0,
                utxo_out_start_index: 0,
                has_unshield: false,
            }]
        } else {
            vec![]
        })
    }
}

struct Node {
    accept: Mutex<bool>,
    index: Mutex<u32>,
}

impl Node {
    fn new(index: u32, accept: bool) -> Self {
        Self {
            accept: Mutex::new(accept),
            index: Mutex::new(index),
        }
    }
}

#[async_trait::async_trait]
impl PoiNodeClient for Node {
    fn list_keys(&self) -> Vec<ListKey> {
        vec![]
    }
    async fn merkletree_leaves(
        &self,
        _: &ListKey,
        _: u64,
        _: u64,
    ) -> Result<Vec<U256>, PoiClientError> {
        unreachable!()
    }
    async fn validate_poi_merkleroots(
        &self,
        _: &ListKey,
        _: &[MerkleRoot],
    ) -> Result<bool, PoiClientError> {
        unreachable!()
    }
    async fn submit_proof(
        &self,
        _: HashMap<ListKey, TransactProofData>,
    ) -> Result<(), PoiClientError> {
        unreachable!()
    }
    async fn validated_txid(&self) -> Result<ValidatedRailgunTxidStatus, PoiClientError> {
        Ok(ValidatedRailgunTxidStatus {
            index: *self.index.lock().unwrap(),
        })
    }
    async fn validate_txid_merkleroot(
        &self,
        _: u32,
        _: u32,
        _: MerkleRoot,
    ) -> Result<bool, PoiClientError> {
        Ok(*self.accept.lock().unwrap())
    }
}

#[tokio::test]
async fn rejected_root_keeps_checkpoint_unchanged_and_retry_restores() {
    let node = Node::new(0, false);
    let mut indexer = TxidIndexer::new(TxidIndexerState::default(), Arc::new(Syncer)).unwrap();
    assert!(indexer.sync_to(10, &node).await.is_err());
    assert_eq!(indexer.state().synced_block, 0);
    assert!(indexer.state().trees.is_empty());
    *node.accept.lock().unwrap() = true;
    indexer.sync_to(10, &node).await.unwrap();
    let restored = TxidIndexer::new(indexer.state(), Arc::new(Syncer)).unwrap();
    assert_eq!(restored.state().synced_block, 10);
    assert_eq!(restored.tree(0).unwrap().leaves_len(), 1);
    assert_eq!(restored.state().txid_to_txid_position.len(), 1);
}

struct TwoSyncer;

#[async_trait::async_trait]
impl TxidSyncer for TwoSyncer {
    async fn latest_block(&self) -> Result<u64, SyncerError> {
        Ok(20)
    }
    async fn sync(&self, from: u64, to: u64) -> Result<Vec<Operation>, SyncerError> {
        Ok([10, 20]
            .into_iter()
            .filter(|block| (from..=to).contains(block))
            .map(|block| Operation {
                block_number: block,
                nullifiers: vec![U256::from(block)],
                commitment_hashes: vec![U256::from(block + 1)],
                bound_params_hash: U256::from(3),
                utxo_tree_in: 0,
                utxo_tree_out: 0,
                utxo_out_start_index: (block / 10 - 1) as u32,
                has_unshield: false,
            })
            .collect())
    }
}

#[tokio::test]
async fn older_target_does_not_regress_a_checkpoint() {
    let node = Node::new(1, true);
    let mut indexer = TxidIndexer::new(TxidIndexerState::default(), Arc::new(TwoSyncer)).unwrap();
    indexer.sync_to(20, &node).await.unwrap();
    let before = indexer.state();
    indexer.sync_to(10, &node).await.unwrap();
    assert_eq!(indexer.state().synced_block, 20);
    assert_eq!(indexer.state().trees[&0].leaves, before.trees[&0].leaves);
    assert_eq!(indexer.state().txid_to_txid_position.len(), 2);
}

#[tokio::test]
async fn older_target_can_finish_previously_pending_operations() {
    let node = Node::new(0, true);
    let mut indexer = TxidIndexer::new(TxidIndexerState::default(), Arc::new(TwoSyncer)).unwrap();
    indexer.sync_to(20, &node).await.unwrap();
    assert_eq!(indexer.state().pending.len(), 1);
    *node.index.lock().unwrap() = 1;
    indexer.sync_to(10, &node).await.unwrap();
    assert_eq!(indexer.state().synced_block, 20);
    assert!(indexer.state().pending.is_empty());
    assert_eq!(indexer.state().trees[&0].leaves.len(), 2);
}
