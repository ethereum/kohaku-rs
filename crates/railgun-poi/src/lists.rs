use std::{collections::HashMap, future::Future};

use ruint::aliases::U256;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{info, warn};

use crate::{
    common::sleep,
    merkle_tree::{MerkleProof, MerkleTreeError, PoiMerkleTree, TOTAL_LEAVES, is_poi_empty_leaf},
    poi::{
        client::{PoiClientError, PoiNodeClient},
        types::{BlindedCommitment, ListKey},
    },
};

/// One request, matching the node's cap on `ppoi_poi_merkletree_leaves`.
const LEAF_PAGE: u64 = 100;
/// Pages applied, validated and saved together. A save writes each touched tree whole (up to
/// 65,536 leaves), so saving after each page made the first copy of
/// a long list slow down as each tree filled. A failed batch costs at most this many pages again.
const BATCH_PAGES: u64 = 20;
/// Leaf requests in flight while a batch is fetched.
const PARALLEL_FETCHES: usize = 4;
/// Extra attempts after the first failure, same budget as the Subsquid syncer.
const MAX_RETRIES: usize = 3;

/// Every configured POI list, stored locally and queried by leaf index.
pub struct PoiLists {
    lists: HashMap<ListKey, PoiList>,
    inactive: HashMap<ListKey, PoiListState>,
}

/// Caller-owned POI list checkpoints. The membership index is rebuilt from trees.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct PoiListsState(pub HashMap<ListKey, PoiListState>);

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct PoiListState {
    pub trees: HashMap<u32, crate::merkle_tree::MerkleTreeState>,
    pub next_index: u64,
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
    #[error("Merkle tree error: {0}")]
    MerkleTree(#[from] MerkleTreeError),
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
    pub fn load(mut state: PoiListsState, list_keys: &[ListKey]) -> Result<Self, MerkleTreeError> {
        let mut lists = HashMap::new();
        for list_key in list_keys {
            lists.insert(
                list_key.clone(),
                PoiList::load(state.0.remove(list_key).unwrap_or_default())?,
            );
        }
        Ok(Self {
            lists,
            inactive: state.0,
        })
    }

    pub fn state(&self) -> PoiListsState {
        let mut state = self.inactive.clone();
        state.extend(
            self.lists
                .iter()
                .map(|(key, list)| (key.clone(), list.state())),
        );
        PoiListsState(state)
    }

    pub fn contains(&self, list_key: &ListKey, commitment: BlindedCommitment) -> bool {
        self.lists
            .get(list_key)
            .is_some_and(|list| list.index.contains_key(&commitment))
    }

    pub fn proof(&self, list_key: &ListKey, commitment: BlindedCommitment) -> Option<MerkleProof> {
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

    /// Copies each list forward from its saved cursor, with bounded parallel page fetches.
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
        use futures::StreamExt;

        if let Some(list) = self.lists.get_mut(list_key) {
            list.caught_up = false;
        }
        // The first batch is one page, so a list that is already caught up costs one request.
        // After a rejected batch, one page at a time keeps the prefix the node accepts.
        let mut batch_pages = 1;
        let mut narrowed = false;
        loop {
            let start = self.next_index(list_key);
            let ranges: Vec<(u64, u64)> = (0..batch_pages)
                .map_while(|i| {
                    let page_start = start.checked_add(i * LEAF_PAGE)?;
                    Some((page_start, page_start.checked_add(LEAF_PAGE)?))
                })
                .collect();
            let Some(&(_, span_end)) = ranges.last() else {
                return Ok(());
            };
            let mut pages =
                futures::stream::iter(ranges.iter().map(|&(page_start, page_end)| async move {
                    let page =
                        retry(|| client.merkletree_leaves(list_key, page_start, page_end)).await;
                    (page_start, page_end, page)
                }))
                .buffered(PARALLEL_FETCHES);

            // Applied in order; the first page that is not full ends the batch.
            // Keep every in-flight page out of the published trees and membership index.
            let mut candidate = self.lists[list_key].stage(start, (span_end - start) as usize);
            let mut roots = Vec::new();
            let mut cursor = start;
            let mut pages_applied = 0;
            let mut stopped = None;
            let mut end_reached = false;
            while let Some((page_start, page_end, page)) = pages.next().await {
                let page = match page {
                    Ok(page) => page,
                    Err(err) => {
                        stopped = Some(PoiSyncError::from(err));
                        break;
                    }
                };
                let expected = (page_end - page_start) as usize;
                let zero_at = page.iter().position(|leaf| is_poi_empty_leaf(*leaf));
                if page.len() < expected && zero_at.is_none() {
                    stopped = Some(PoiSyncError::ShortPage {
                        list_key: list_key.clone(),
                        start: page_start,
                        end: page_end,
                        got: page.len(),
                    });
                    break;
                }
                let real_len = zero_at.unwrap_or(page.len());
                let real = &page[..real_len];
                if real.is_empty() {
                    end_reached = true;
                    break;
                }

                candidate.insert_range(page_start, real)?;
                // Validate every page's roots, preserving the batched copy's trust boundary.
                for number in touched_trees(page_start, real_len) {
                    roots.push(candidate.trees[&number].root()?);
                }
                cursor = page_start + real_len as u64;
                pages_applied += 1;
                if real_len < page.len() {
                    end_reached = true;
                    break;
                }
            }
            drop(pages);

            if pages_applied == 0 {
                if let Some(err) = stopped {
                    return Err(err);
                }
                if end_reached {
                    self.lists
                        .get_mut(list_key)
                        .expect("POI list loaded at startup")
                        .caught_up = true;
                    info!("POI list {list_key} is synced at leaf {start}");
                }
                return Ok(());
            }

            if !retry(|| client.validate_poi_merkleroots(list_key, &roots)).await? {
                if pages_applied > 1 {
                    batch_pages = 1;
                    narrowed = true;
                    continue;
                }
                return Err(PoiSyncError::RootRejected {
                    list_key: list_key.clone(),
                });
            }

            // Publish only trees actually touched before the batch stopped.
            let tree_numbers = touched_trees(start, (cursor - start) as usize);
            candidate
                .trees
                .retain(|number, _| tree_numbers.contains(number));
            // No await between validation and publication.
            let list = self
                .lists
                .get_mut(list_key)
                .expect("POI list loaded at startup");
            list.trees.extend(candidate.trees);
            list.index.extend(candidate.index);
            list.next_index = cursor;
            info!("Synced POI list {list_key} through leaf {cursor}");

            if let Some(err) = stopped {
                return Err(err);
            }
            if end_reached {
                self.lists
                    .get_mut(list_key)
                    .expect("POI list loaded at startup")
                    .caught_up = true;
                return Ok(());
            }
            if !narrowed {
                batch_pages = BATCH_PAGES;
            }
        }
    }
}

impl PoiList {
    /// Clone only trees touched by this batch; the full commitment index is not cloned.
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

    fn load(state: PoiListState) -> Result<Self, MerkleTreeError> {
        let next_index = state.next_index;
        let mut trees = HashMap::new();
        let mut index = HashMap::new();
        for (tree_number, tree_state) in state.trees {
            let tree = PoiMerkleTree::from_state(tree_state)?;
            for (leaf_index, leaf) in tree.leaves().iter().copied().enumerate() {
                index.insert(
                    BlindedCommitment::from(leaf),
                    (tree_number, leaf_index as u32),
                );
            }
            trees.insert(tree_number, tree);
        }
        Ok(Self {
            trees,
            index,
            next_index,
            caught_up: false,
        })
    }

    fn state(&self) -> PoiListState {
        PoiListState {
            trees: self
                .trees
                .iter()
                .map(|(number, tree)| (*number, tree.state()))
                .collect(),
            next_index: self.next_index,
        }
    }

    fn insert_range(&mut self, start: u64, leaves: &[U256]) -> Result<(), MerkleTreeError> {
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
            tree.insert_leaves(chunk, leaf_index as usize)?;
            for (i, leaf) in chunk.iter().copied().enumerate() {
                self.index.insert(
                    BlindedCommitment::from(leaf),
                    (tree_number, leaf_index + i as u32),
                );
            }
            offset += count;
        }
        Ok(())
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
#[path = "lists_state_tests.rs"]
mod tests;
