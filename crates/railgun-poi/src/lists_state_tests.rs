use super::*;
use crate::{
    merkle_tree::{MerkleRoot, PoiHasher},
    poi::types::TransactProofData,
};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
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
    pause_root_after: AtomicUsize,
    pause_fetch_at: AtomicUsize,
    entered_fetch: tokio::sync::Notify,
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
            pause_root_after: AtomicUsize::new(usize::MAX),
            pause_fetch_at: AtomicUsize::new(usize::MAX),
            entered_fetch: tokio::sync::Notify::new(),
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
        if start_index as usize == self.pause_fetch_at.load(Ordering::Relaxed) {
            self.entered_fetch.notify_one();
            std::future::pending::<()>().await;
        }
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
        if self.pause_root.load(Ordering::Relaxed)
            || self.root_checks.load(Ordering::Relaxed)
                >= self.pause_root_after.load(Ordering::Relaxed)
        {
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
    let key = list_key();
    let mut leaves: Vec<U256> = (1..=101u32).map(U256::from).collect();
    let node = FakeNode::new(leaves.clone(), true);
    let mut lists = PoiLists::load(PoiListsState::default(), &[key.clone()]).unwrap();

    lists.sync(&node).await.unwrap();
    assert_eq!(lists.next_index(&key), 101);
    assert!(lists.caught_up(&key));
    let commitment = BlindedCommitment::from(U256::from(2u8));
    let proof = lists.proof(&key, commitment).unwrap();
    assert!(proof.verify::<PoiHasher>());

    let saved: PoiListsState =
        serde_json::from_slice(&serde_json::to_vec(&lists.state()).unwrap()).unwrap();
    let reloaded = PoiLists::load(saved, &[key.clone()]).unwrap();
    assert_eq!(reloaded.next_index(&key), 101);
    assert!(!reloaded.caught_up(&key));
    assert!(
        reloaded
            .proof(&key, commitment)
            .unwrap()
            .verify::<PoiHasher>()
    );

    leaves.push(U256::from(102u32));
    leaves.push(U256::from(103u32));
    node.set_leaves(leaves);
    lists.sync(&node).await.unwrap();
    assert_eq!(lists.next_index(&key), 103);
    assert!(lists.contains(&key, BlindedCommitment::from(U256::from(103u32))));
}

#[tokio::test]
async fn rejected_root_does_not_advance_the_cursor() {
    let key = list_key();
    let node = FakeNode::new((1..=3u32).map(U256::from).collect(), false);
    let mut lists = PoiLists::load(PoiListsState::default(), &[key.clone()]).unwrap();

    let err = lists.sync(&node).await.unwrap_err();
    assert!(matches!(err, PoiSyncError::RootRejected { .. }));
    assert_eq!(lists.next_index(&key), 0);
    assert!(!lists.caught_up(&key));
    assert!(
        lists
            .proof(&key, BlindedCommitment::from(U256::from(1u8)))
            .is_none()
    );

    let reloaded = PoiLists::load(lists.state(), &[key.clone()]).unwrap();
    assert_eq!(reloaded.next_index(&key), 0);
    assert!(!reloaded.contains(&key, BlindedCommitment::from(U256::from(1u8))));
}

#[tokio::test]
async fn a_rejected_later_page_keeps_the_prefix_but_not_caught_up() {
    let key = list_key();
    let node = FakeNode::new((1..=101u32).map(U256::from).collect(), true);
    node.reject_after(1);
    let mut lists = PoiLists::load(PoiListsState::default(), &[key.clone()]).unwrap();

    let err = lists.sync(&node).await.unwrap_err();
    assert!(matches!(err, PoiSyncError::RootRejected { .. }));
    assert_eq!(lists.next_index(&key), 100);
    assert!(!lists.caught_up(&key));
    assert!(lists.contains(&key, BlindedCommitment::from(U256::from(1u8))));
    assert!(!lists.contains(&key, BlindedCommitment::from(U256::from(101u32))));
}

#[tokio::test]
async fn an_empty_page_is_not_the_end_of_the_list() {
    let key = list_key();
    let node = FakeNode::empty();
    let mut lists = PoiLists::load(PoiListsState::default(), &[key.clone()]).unwrap();

    let err = lists.sync(&node).await.unwrap_err();
    assert!(matches!(err, PoiSyncError::ShortPage { got: 0, .. }));
    assert_eq!(lists.next_index(&key), 0);
    assert!(!lists.caught_up(&key));
}

#[tokio::test]
async fn a_short_page_without_a_zero_is_not_applied() {
    let key = list_key();
    let node = FakeNode::exact((1..=3u32).map(U256::from).collect());
    let mut lists = PoiLists::load(PoiListsState::default(), &[key.clone()]).unwrap();

    let err = lists.sync(&node).await.unwrap_err();
    assert!(matches!(err, PoiSyncError::ShortPage { got: 3, .. }));
    assert_eq!(lists.next_index(&key), 0);
    assert!(!lists.caught_up(&key));
    assert!(!lists.contains(&key, BlindedCommitment::from(U256::from(1u8))));
}

#[tokio::test]
async fn cancellation_during_validation_does_not_publish_a_page() {
    let key = list_key();
    let node = FakeNode::new(vec![U256::from(1)], true);
    node.pause_root.store(true, Ordering::Relaxed);
    let mut lists = PoiLists::load(PoiListsState::default(), &[key.clone()]).unwrap();
    {
        let future = lists.sync(&node);
        tokio::pin!(future);
        tokio::select! {
            result = &mut future => panic!("unexpected completion: {result:?}"),
            () = node.entered_root.notified() => {},
        }
    }
    assert_eq!(lists.next_index(&key), 0);
    assert!(lists.state().0[&key].trees.is_empty());
}

#[tokio::test]
async fn cancellation_during_later_fetch_keeps_the_validated_prefix() {
    let key = list_key();
    let node = FakeNode::new((1..=201u32).map(U256::from).collect(), true);
    node.pause_fetch_at.store(100, Ordering::Relaxed);
    let mut lists = PoiLists::load(PoiListsState::default(), &[key.clone()]).unwrap();
    {
        let future = lists.sync(&node);
        tokio::pin!(future);
        tokio::select! {
            result = &mut future => panic!("unexpected completion: {result:?}"),
            () = node.entered_fetch.notified() => {},
        }
    }
    assert_eq!(lists.next_index(&key), 100);
    let reloaded = PoiLists::load(lists.state(), &[key.clone()]).unwrap();
    assert_eq!(reloaded.next_index(&key), 100);
    assert!(reloaded.contains(&key, BlindedCommitment::from(U256::from(1))));
    assert!(!reloaded.contains(&key, BlindedCommitment::from(U256::from(101))));
}

#[test]
fn unconfigured_list_state_survives_a_session() {
    let key = list_key();
    let inactive: ListKey = "another-list".into();
    let checkpoint = PoiListsState(
        [(
            inactive.clone(),
            PoiListState {
                trees: HashMap::new(),
                next_index: 42,
            },
        )]
        .into(),
    );
    let lists = PoiLists::load(checkpoint, &[key]).unwrap();
    assert_eq!(lists.state().0[&inactive].next_index, 42);
}
