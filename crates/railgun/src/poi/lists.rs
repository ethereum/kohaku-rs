use std::{collections::HashMap, future::Future};

use kohaku_kv_store::Store;
use ruint::aliases::U256;
use thiserror::Error;
use tracing::{info, warn};

use crate::{
    common::sleep,
    merkle_tree::{MerkleRoot, PoiMerkleProof, PoiMerkleTree, TOTAL_LEAVES, is_poi_empty_leaf},
    poi::{
        client::{PoiClientError, PoiNodeClient},
        types::{BlindedCommitment, ListKey},
    },
    railgun_database::{DatabaseError, RailgunDB, write_poi_snapshot},
};

/// One request, matching the node's cap on `ppoi_poi_merkletree_leaves`.
const LEAF_PAGE: u64 = 100;
/// Extra attempts after the first failure, same budget as the Subsquid syncer.
const MAX_RETRIES: usize = 3;

/// Every configured POI list, stored locally and queried by leaf index.
pub struct PoiLists {
    db: Store,
    lists: HashMap<ListKey, PoiList>,
}

struct PoiList {
    trees: HashMap<u32, PoiMerkleTree>,
    /// Blinded commitment to its position. Built from the trees, not stored.
    index: HashMap<BlindedCommitment, (u32, u32)>,
    next_index: u64,
    /// True only after this process has copied through the zero leaf. Not stored:
    /// a restart has not re-checked the tail, so absence is not yet `Missing`.
    caught_up: bool,
}

#[derive(Debug, Error)]
pub enum PoiSyncError {
    #[error("POI client error: {0}")]
    Client(#[from] PoiClientError),
    #[error("Database error: {0}")]
    Database(#[from] DatabaseError),
    #[error("POI node rejected merkle roots for list {list_key}")]
    RootRejected { list_key: ListKey },
    /// The node owes a full page, or a shorter one that ends in the POI zero.
    #[error("POI list {list_key} returned {got} leaves for {start}..{end} with no zero leaf")]
    ShortPage {
        list_key: ListKey,
        start: u64,
        end: u64,
        got: usize,
    },
}

impl PoiLists {
    pub async fn load(db: Store, list_keys: &[ListKey]) -> Result<Self, DatabaseError> {
        let mut lists = HashMap::new();
        for list_key in list_keys {
            lists.insert(list_key.clone(), PoiList::load(&db, list_key).await?);
        }
        Ok(Self { db, lists })
    }

    pub fn contains(&self, list_key: &ListKey, commitment: BlindedCommitment) -> bool {
        self.lists
            .get(list_key)
            .is_some_and(|list| list.index.contains_key(&commitment))
    }

    pub fn proof(
        &self,
        list_key: &ListKey,
        commitment: BlindedCommitment,
    ) -> Option<PoiMerkleProof> {
        let list = self.lists.get(list_key)?;
        let (tree_number, leaf_index) = list.index.get(&commitment).copied()?;
        let tree = list.trees.get(&tree_number)?;
        match tree.generate_proof_at(leaf_index as usize) {
            Ok(proof) => Some(proof),
            Err(err) => {
                warn!("POI proof for list {list_key} failed: {err}");
                None
            }
        }
    }

    pub fn next_index(&self, list_key: &ListKey) -> u64 {
        self.lists
            .get(list_key)
            .map(|list| list.next_index)
            .unwrap_or(0)
    }

    /// Whether the latest sync of this list reached its zero leaf.
    pub fn caught_up(&self, list_key: &ListKey) -> bool {
        self.lists.get(list_key).is_some_and(|list| list.caught_up)
    }

    /// Copies each list forward from its saved cursor. One request at a time.
    pub async fn sync(&mut self, client: &impl PoiNodeClient) -> Result<(), PoiSyncError> {
        let list_keys: Vec<ListKey> = self.lists.keys().cloned().collect();
        for list_key in list_keys {
            self.sync_list(client, &list_key).await?;
        }
        Ok(())
    }

    async fn sync_list(
        &mut self,
        client: &impl PoiNodeClient,
        list_key: &ListKey,
    ) -> Result<(), PoiSyncError> {
        if let Some(list) = self.lists.get_mut(list_key) {
            list.caught_up = false;
        }
        loop {
            let start = self.next_index(list_key);
            let Some(end) = start.checked_add(LEAF_PAGE) else {
                return Ok(());
            };

            let page = retry(|| client.merkletree_leaves(list_key, start, end)).await?;
            let expected = (end - start) as usize;
            let zero_at = page.iter().position(|leaf| is_poi_empty_leaf(*leaf));
            if page.len() < expected && zero_at.is_none() {
                return Err(PoiSyncError::ShortPage {
                    list_key: list_key.clone(),
                    start,
                    end,
                    got: page.len(),
                });
            }
            let real_len = zero_at.unwrap_or(page.len());
            let real = &page[..real_len];
            if real.is_empty() {
                self.lists
                    .get_mut(list_key)
                    .expect("POI list loaded at startup")
                    .caught_up = true;
                info!("POI list {list_key} is synced at leaf {start}");
                return Ok(());
            }

            // Keep unvalidated/unpersisted leaves out of the live index. Dropping this future
            // during either await discards the candidate, not the last published page.
            let mut candidate = self.lists[list_key].stage(start, real.len());
            candidate.insert_range(start, real);
            let roots: Vec<MerkleRoot> =
                candidate.trees.values().map(PoiMerkleTree::root).collect();
            if !retry(|| client.validate_poi_merkleroots(list_key, &roots)).await? {
                return Err(PoiSyncError::RootRejected {
                    list_key: list_key.clone(),
                });
            }

            let synced_through = start + real_len as u64;
            let trees: Vec<_> = candidate
                .trees
                .iter()
                .map(|(number, tree)| (*number, tree.state()))
                .collect();
            write_poi_snapshot(&self.db, list_key, synced_through, &trees).await?;

            // No await between durable save and publication. A cancelled save may have committed;
            // the next sync can replay the same validated page, or a reopen can load it.
            let list = self
                .lists
                .get_mut(list_key)
                .expect("POI list loaded at startup");
            list.trees.extend(candidate.trees);
            list.index.extend(candidate.index);
            list.next_index = synced_through;
            info!("Synced POI list {list_key} through leaf {synced_through}");

            if real_len < page.len() {
                self.lists
                    .get_mut(list_key)
                    .expect("POI list loaded at startup")
                    .caught_up = true;
                return Ok(());
            }
        }
    }
}

impl PoiList {
    /// Clone only trees touched by this page; the full commitment index is not cloned.
    /// The candidate index records new positions only and is merged after the save succeeds.
    fn stage(&self, start: u64, count: usize) -> Self {
        let trees = touched_trees(start, count)
            .into_iter()
            .map(|number| {
                let tree = self
                    .trees
                    .get(&number)
                    .cloned()
                    .unwrap_or_else(|| PoiMerkleTree::new(number));
                (number, tree)
            })
            .collect();
        Self {
            trees,
            index: HashMap::new(),
            next_index: start,
            caught_up: false,
        }
    }

    async fn load(db: &Store, list_key: &ListKey) -> Result<Self, DatabaseError> {
        let next_index = db.get_poi_sync(list_key).await?.next_index;
        let mut trees = HashMap::new();
        let mut index = HashMap::new();
        let mut tree_number = 0u32;
        loop {
            let Some(state) = db.get_poi_tree(list_key, tree_number).await? else {
                break;
            };
            let tree = PoiMerkleTree::from_state(state);
            for (leaf_index, leaf) in tree.leaves().iter().copied().enumerate() {
                index.insert(
                    BlindedCommitment::from(leaf),
                    (tree_number, leaf_index as u32),
                );
            }
            trees.insert(tree_number, tree);
            tree_number = match tree_number.checked_add(1) {
                Some(next) => next,
                None => break,
            };
        }
        Ok(Self {
            trees,
            index,
            next_index,
            caught_up: false,
        })
    }

    fn insert_range(&mut self, start: u64, leaves: &[U256]) {
        let mut offset = 0usize;
        while offset < leaves.len() {
            let global = start + offset as u64;
            let tree_number = (global / TOTAL_LEAVES as u64) as u32;
            let leaf_index = (global % TOTAL_LEAVES as u64) as u32;
            let room = (TOTAL_LEAVES - leaf_index) as usize;
            let count = (leaves.len() - offset).min(room);
            let chunk = &leaves[offset..offset + count];
            let tree = self
                .trees
                .entry(tree_number)
                .or_insert_with(|| PoiMerkleTree::new(tree_number));
            tree.insert_leaves(chunk, leaf_index as usize);
            for (i, leaf) in chunk.iter().copied().enumerate() {
                self.index.insert(
                    BlindedCommitment::from(leaf),
                    (tree_number, leaf_index + i as u32),
                );
            }
            offset += count;
        }
    }
}

fn touched_trees(start: u64, count: usize) -> Vec<u32> {
    if count == 0 {
        return Vec::new();
    }
    let last = start + count as u64 - 1;
    let first_tree = (start / TOTAL_LEAVES as u64) as u32;
    let last_tree = (last / TOTAL_LEAVES as u64) as u32;
    (first_tree..=last_tree).collect()
}

async fn retry<T, F, Fut>(mut op: F) -> Result<T, PoiClientError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, PoiClientError>>,
{
    let mut attempt = 0;
    loop {
        match op().await {
            Ok(value) => return Ok(value),
            Err(err) => {
                attempt += 1;
                if attempt > MAX_RETRIES {
                    return Err(err);
                }
                warn!("POI request failed (attempt {attempt}/{MAX_RETRIES}): {err}");
                sleep(web_time::Duration::from_secs(1)).await;
            }
        }
    }
}

#[cfg(all(test, native))]
mod tests {
    use std::sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    use super::*;
    use crate::poi::types::TransactProofData;
    use kohaku_kv_store::{
        backend::{KvStoreBackend, StoreError},
        memory::MemoryStore,
    };

    #[derive(Clone, Copy)]
    enum LeafPages {
        /// Full requested range, POI zero past the stored leaves.
        Padded,
        Empty,
        /// Stored leaves only, no zero padding.
        Exact,
    }

    struct FakeNode {
        leaves: Mutex<Vec<U256>>,
        accept_roots: AtomicBool,
        /// Accept this many root checks, then reject. `None` follows `accept_roots`.
        accept_prefix: Mutex<Option<usize>>,
        root_checks: AtomicUsize,
        pages: LeafPages,
        pause_root: AtomicBool,
        entered_root: tokio::sync::Notify,
    }

    impl FakeNode {
        fn new(leaves: Vec<U256>, accept_roots: bool) -> Self {
            Self::with_pages(leaves, accept_roots, LeafPages::Padded)
        }

        fn empty() -> Self {
            Self::with_pages(Vec::new(), true, LeafPages::Empty)
        }

        fn exact(leaves: Vec<U256>) -> Self {
            Self::with_pages(leaves, true, LeafPages::Exact)
        }

        fn with_pages(leaves: Vec<U256>, accept_roots: bool, pages: LeafPages) -> Self {
            Self {
                leaves: Mutex::new(leaves),
                accept_roots: AtomicBool::new(accept_roots),
                accept_prefix: Mutex::new(None),
                root_checks: AtomicUsize::new(0),
                pages,
                pause_root: AtomicBool::new(false),
                entered_root: tokio::sync::Notify::new(),
            }
        }

        fn reject_after(&self, accepted: usize) {
            *self.accept_prefix.lock().unwrap() = Some(accepted);
        }

        fn set_leaves(&self, leaves: Vec<U256>) {
            *self.leaves.lock().unwrap() = leaves;
        }
    }

    #[cfg_attr(native, async_trait::async_trait)]
    impl PoiNodeClient for FakeNode {
        fn list_keys(&self) -> Vec<ListKey> {
            vec![ListKey::from("list")]
        }

        async fn merkletree_leaves(
            &self,
            _list_key: &ListKey,
            start_index: u64,
            end_index: u64,
        ) -> Result<Vec<U256>, PoiClientError> {
            match self.pages {
                LeafPages::Empty => Ok(Vec::new()),
                LeafPages::Exact => {
                    let leaves = self.leaves.lock().unwrap();
                    let start = start_index as usize;
                    let end = (end_index as usize).min(leaves.len());
                    if start >= end {
                        Ok(Vec::new())
                    } else {
                        Ok(leaves[start..end].to_vec())
                    }
                }
                LeafPages::Padded => {
                    let leaves = self.leaves.lock().unwrap();
                    let zero = PoiMerkleTree::zero();
                    Ok((start_index..end_index)
                        .map(|index| leaves.get(index as usize).copied().unwrap_or(zero))
                        .collect())
                }
            }
        }

        async fn validate_poi_merkleroots(
            &self,
            _list_key: &ListKey,
            _roots: &[MerkleRoot],
        ) -> Result<bool, PoiClientError> {
            if self.pause_root.load(Ordering::Relaxed) {
                self.entered_root.notify_one();
                std::future::pending::<()>().await;
            }
            let n = self.root_checks.fetch_add(1, Ordering::Relaxed);
            if let Some(prefix) = *self.accept_prefix.lock().unwrap() {
                return Ok(n < prefix);
            }
            Ok(self.accept_roots.load(Ordering::Relaxed))
        }

        async fn submit_proof(
            &self,
            _proof_data: HashMap<ListKey, TransactProofData>,
        ) -> Result<(), PoiClientError> {
            unimplemented!()
        }

        async fn validated_txid(
            &self,
        ) -> Result<crate::poi::types::ValidatedRailgunTxidStatus, PoiClientError> {
            unimplemented!()
        }

        async fn validate_txid_merkleroot(
            &self,
            _tree: u32,
            _index: u32,
            _merkleroot: MerkleRoot,
        ) -> Result<bool, PoiClientError> {
            unimplemented!()
        }
    }

    fn list_key() -> ListKey {
        ListKey::from("list")
    }

    #[tokio::test]
    async fn sync_stops_at_the_zero_leaf_and_resumes() {
        let db = Store::create();
        let key = list_key();
        let mut leaves: Vec<U256> = (1..=101u32).map(U256::from).collect();
        let node = FakeNode::new(leaves.clone(), true);
        let mut lists = PoiLists::load(db.clone(), &[key.clone()]).await.unwrap();

        lists.sync(&node).await.unwrap();
        assert_eq!(lists.next_index(&key), 101);
        assert!(lists.caught_up(&key));
        let commitment = BlindedCommitment::from(U256::from(2u8));
        let proof = lists.proof(&key, commitment).unwrap();
        assert!(proof.verify());

        let reloaded = PoiLists::load(db.clone(), &[key.clone()]).await.unwrap();
        assert_eq!(reloaded.next_index(&key), 101);
        assert!(!reloaded.caught_up(&key));
        assert!(reloaded.proof(&key, commitment).unwrap().verify());

        leaves.push(U256::from(102u32));
        leaves.push(U256::from(103u32));
        node.set_leaves(leaves);
        lists.sync(&node).await.unwrap();
        assert_eq!(lists.next_index(&key), 103);
        assert!(lists.contains(&key, BlindedCommitment::from(U256::from(103u32))));
    }

    #[tokio::test]
    async fn rejected_root_does_not_advance_the_cursor() {
        let db = Store::create();
        let key = list_key();
        let node = FakeNode::new((1..=3u32).map(U256::from).collect(), false);
        let mut lists = PoiLists::load(db.clone(), &[key.clone()]).await.unwrap();

        let err = lists.sync(&node).await.unwrap_err();
        assert!(matches!(err, PoiSyncError::RootRejected { .. }));
        assert_eq!(lists.next_index(&key), 0);
        assert!(!lists.caught_up(&key));
        assert!(
            lists
                .proof(&key, BlindedCommitment::from(U256::from(1u8)))
                .is_none()
        );

        let reloaded = PoiLists::load(db, &[key.clone()]).await.unwrap();
        assert_eq!(reloaded.next_index(&key), 0);
        assert!(!reloaded.contains(&key, BlindedCommitment::from(U256::from(1u8))));
    }

    #[tokio::test]
    async fn a_rejected_later_page_keeps_the_prefix_but_not_caught_up() {
        let db = Store::create();
        let key = list_key();
        let node = FakeNode::new((1..=101u32).map(U256::from).collect(), true);
        node.reject_after(1);
        let mut lists = PoiLists::load(db, &[key.clone()]).await.unwrap();

        let err = lists.sync(&node).await.unwrap_err();
        assert!(matches!(err, PoiSyncError::RootRejected { .. }));
        assert_eq!(lists.next_index(&key), 100);
        assert!(!lists.caught_up(&key));
        assert!(lists.contains(&key, BlindedCommitment::from(U256::from(1u8))));
        assert!(!lists.contains(&key, BlindedCommitment::from(U256::from(101u32))));
    }

    #[tokio::test]
    async fn an_empty_page_is_not_the_end_of_the_list() {
        let db = Store::create();
        let key = list_key();
        let node = FakeNode::empty();
        let mut lists = PoiLists::load(db, &[key.clone()]).await.unwrap();

        let err = lists.sync(&node).await.unwrap_err();
        assert!(matches!(err, PoiSyncError::ShortPage { got: 0, .. }));
        assert_eq!(lists.next_index(&key), 0);
        assert!(!lists.caught_up(&key));
    }

    #[tokio::test]
    async fn a_short_page_without_a_zero_is_not_applied() {
        let db = Store::create();
        let key = list_key();
        let node = FakeNode::exact((1..=3u32).map(U256::from).collect());
        let mut lists = PoiLists::load(db, &[key.clone()]).await.unwrap();

        let err = lists.sync(&node).await.unwrap_err();
        assert!(matches!(err, PoiSyncError::ShortPage { got: 3, .. }));
        assert_eq!(lists.next_index(&key), 0);
        assert!(!lists.caught_up(&key));
        assert!(!lists.contains(&key, BlindedCommitment::from(U256::from(1u8))));
    }

    struct FailWrites {
        inner: MemoryStore,
    }
    #[async_trait::async_trait]
    impl KvStoreBackend for FailWrites {
        async fn get_batch(&self, keys: &[&[u8]]) -> Result<Vec<Option<Vec<u8>>>, StoreError> {
            self.inner.get_batch(keys).await
        }
        async fn put_batch(&self, _: &[(&[u8], &[u8])]) -> Result<(), StoreError> {
            Err(StoreError::from(
                Box::new(std::io::Error::other("injected POI snapshot failure"))
                    as Box<dyn std::error::Error + Send + Sync>,
            ))
        }
        async fn delete_batch(&self, keys: &[&[u8]]) -> Result<(), StoreError> {
            self.inner.delete_batch(keys).await
        }
    }

    #[tokio::test]
    async fn a_failed_save_rolls_the_page_back() {
        let db = Store::new(FailWrites {
            inner: MemoryStore::new(),
        });
        let kept = db.clone();
        let key = list_key();
        let node = FakeNode::new((1..=3u32).map(U256::from).collect(), true);
        let mut lists = PoiLists::load(db, &[key.clone()]).await.unwrap();

        let err = lists.sync(&node).await.unwrap_err();
        assert!(matches!(err, PoiSyncError::Database(_)));
        assert_eq!(lists.next_index(&key), 0);
        assert!(!lists.caught_up(&key));
        assert!(!lists.contains(&key, BlindedCommitment::from(U256::from(1u8))));

        let reloaded = PoiLists::load(kept, &[key.clone()]).await.unwrap();
        assert_eq!(reloaded.next_index(&key), 0);
        assert!(!reloaded.contains(&key, BlindedCommitment::from(U256::from(1u8))));
    }
    #[tokio::test]
    async fn cancelled_root_validation_does_not_publish_unvalidated_leaves() {
        let db = Store::create();
        let key = list_key();
        let old = BlindedCommitment::from(U256::from(1));
        let new = BlindedCommitment::from(U256::from(2));
        let node = FakeNode::new(vec![U256::from(1)], true);
        let mut lists = PoiLists::load(db.clone(), &[key.clone()]).await.unwrap();
        lists.sync(&node).await.unwrap();
        node.set_leaves(vec![U256::from(1), U256::from(2)]);
        node.pause_root.store(true, Ordering::Relaxed);
        let mut sync = Box::pin(lists.sync(&node));
        tokio::select! {
            result = &mut sync => panic!("root validation should block: {result:?}"),
            _ = node.entered_root.notified() => {},
        }
        drop(sync);
        assert_eq!(lists.next_index(&key), 1);
        assert!(!lists.caught_up(&key));
        assert!(lists.contains(&key, old));
        assert!(!lists.contains(&key, new));
        assert!(lists.proof(&key, new).is_none());
        let reloaded = PoiLists::load(db, &[key.clone()]).await.unwrap();
        assert_eq!(reloaded.next_index(&key), 1);
        assert!(!reloaded.contains(&key, new));
        node.pause_root.store(false, Ordering::Relaxed);
        lists.sync(&node).await.unwrap();
        assert_eq!(lists.next_index(&key), 2);
        assert!(lists.proof(&key, new).unwrap().verify());
    }
    struct PausingWrites {
        inner: std::sync::Arc<MemoryStore>,
        paused: std::sync::Arc<AtomicBool>,
        entered: std::sync::Arc<tokio::sync::Notify>,
        after_commit: bool,
    }

    #[async_trait::async_trait]
    impl KvStoreBackend for PausingWrites {
        async fn get_batch(&self, keys: &[&[u8]]) -> Result<Vec<Option<Vec<u8>>>, StoreError> {
            self.inner.get_batch(keys).await
        }
        async fn put_batch(&self, values: &[(&[u8], &[u8])]) -> Result<(), StoreError> {
            if self.after_commit {
                self.inner.put_batch(values).await?;
            }
            if self.paused.load(Ordering::Relaxed) {
                self.entered.notify_one();
                std::future::pending::<()>().await;
            }
            if !self.after_commit {
                self.inner.put_batch(values).await?;
            }
            Ok(())
        }
        async fn delete_batch(&self, keys: &[&[u8]]) -> Result<(), StoreError> {
            self.inner.delete_batch(keys).await
        }
    }

    async fn cancel_save(after_commit: bool) {
        let paused = std::sync::Arc::new(AtomicBool::new(false));
        let entered = std::sync::Arc::new(tokio::sync::Notify::new());
        let db = Store::new(PausingWrites {
            inner: std::sync::Arc::new(MemoryStore::new()),
            paused: paused.clone(),
            entered: entered.clone(),
            after_commit,
        });
        let key = list_key();
        let old = BlindedCommitment::from(U256::from(1));
        let new = BlindedCommitment::from(U256::from(2));
        let node = FakeNode::new(vec![U256::from(1)], true);
        let mut lists = PoiLists::load(db.clone(), &[key.clone()]).await.unwrap();
        lists.sync(&node).await.unwrap();
        node.set_leaves(vec![U256::from(1), U256::from(2)]);
        paused.store(true, Ordering::Relaxed);
        let mut sync = Box::pin(lists.sync(&node));
        tokio::select! {
            result = &mut sync => panic!("save should block: {result:?}"),
            _ = entered.notified() => {},
        }
        drop(sync);
        assert_eq!(lists.next_index(&key), 1);
        assert!(!lists.caught_up(&key));
        assert!(lists.contains(&key, old));
        assert!(!lists.contains(&key, new));
        assert!(lists.proof(&key, new).is_none());
        let reloaded = PoiLists::load(db.clone(), &[key.clone()]).await.unwrap();
        assert_eq!(reloaded.next_index(&key), if after_commit { 2 } else { 1 });
        assert_eq!(reloaded.contains(&key, new), after_commit);
        // A committed-but-unacknowledged snapshot may be ahead on disk. Replaying from the
        // published in-memory prefix must remain safe and must not duplicate the page.
        paused.store(false, Ordering::Relaxed);
        lists.sync(&node).await.unwrap();
        assert_eq!(lists.next_index(&key), 2);
        assert!(lists.caught_up(&key));
        assert!(lists.proof(&key, new).unwrap().verify());
        let reloaded = PoiLists::load(db, &[key.clone()]).await.unwrap();
        assert_eq!(reloaded.next_index(&key), 2);
        assert!(reloaded.proof(&key, new).unwrap().verify());
    }

    #[tokio::test]
    async fn cancelled_save_does_not_publish_the_uncommitted_page() {
        cancel_save(false).await;
    }

    #[tokio::test]
    async fn lost_save_ack_can_replay_from_the_published_prefix() {
        cancel_save(true).await;
    }
}
