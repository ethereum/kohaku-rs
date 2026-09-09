use ruint::aliases::U256;

use crate::kv::KvStore;

const LATEST_BLOCK_KEY: &[u8] = b"latest_block";

pub trait IndexerStore: KvStore {
    async fn latest_block(&self) -> u64 {
        self.get(LATEST_BLOCK_KEY).await.map_or(0, |v| {
            u64::from_be_bytes(v.try_into().expect("latest block is 8 bytes"))
        })
    }

    /// Atomically writes `latest_block` and `nullifiers` to the store.
    async fn commit(&self, latest_block: u64, nullifiers: &[U256]) {
        let latest_block_bytes = latest_block.to_be_bytes();

        let mut items = nullifiers
            .iter()
            .map(|n| (n.as_le_slice(), n.as_le_slice()))
            .collect::<Vec<_>>();
        items.push((LATEST_BLOCK_KEY, &latest_block_bytes));

        self.batch_put(&items).await;
    }
}

impl<T: KvStore + ?Sized> IndexerStore for T {}
