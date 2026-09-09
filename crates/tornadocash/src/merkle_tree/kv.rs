use ruint::aliases::U256;

use crate::kv::KvStore;

const LEAF_COUNT_KEY: &[u8] = b"leaf_count";

pub trait MerkleTreeStore: KvStore {
    /// Reads the hash stored at `(level, index)`, if any.
    async fn node(&self, level: u8, index: u64) -> Option<U256> {
        let key = node_key(level, index);
        self.get(&key).await.map(|v| U256::from_le_slice(&v))
    }

    /// Returns the number of leaves currently committed.
    async fn leaf_count(&self) -> u64 {
        self.get(LEAF_COUNT_KEY)
            .await
            .map_or(0, |v| u64::from_be_bytes(v.try_into().expect("leaf count is 8 bytes")))
    }

    /// Atomically writes `nodes` and `leaf_count`.
    async fn commit(&self, leaf_count: u64, nodes: &[(u8, u64, U256)]) {
        let keys: Vec<[u8; 9]> = nodes
            .iter()
            .map(|(level, index, _)| node_key(*level, *index))
            .collect();
        let count_bytes = leaf_count.to_be_bytes();

        let mut items: Vec<(&[u8], &[u8])> = keys
            .iter()
            .zip(nodes)
            .map(|(key, (_, _, hash))| (key.as_ref(), hash.as_le_slice()))
            .collect();
        items.push((LEAF_COUNT_KEY, &count_bytes));

        self.batch_put(&items).await;
    }
}

impl<T: KvStore + ?Sized> MerkleTreeStore for T {}

fn node_key(level: u8, index: u64) -> [u8; 9] {
    let mut key = [0u8; 9];
    key[0] = level;
    key[1..9].copy_from_slice(&index.to_be_bytes());
    key
}
