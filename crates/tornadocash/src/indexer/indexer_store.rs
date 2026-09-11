use kohaku_kv_store::Store;

const LATEST_BLOCK_KEY: &[u8] = b"latest_block";

#[async_trait::async_trait]
pub trait IndexerStoreExt {
    async fn latest_block(&self) -> u64;
    async fn commit(&self, latest_block: u64);
}

#[async_trait::async_trait]
impl IndexerStoreExt for Store {
    async fn latest_block(&self) -> u64 {
        self.get(LATEST_BLOCK_KEY).await.map_or(0, |v| {
            u64::from_be_bytes(v.try_into().expect("latest block is 8 bytes"))
        })
    }

    /// Atomically writes `latest_block` to the store.
    async fn commit(&self, latest_block: u64) {
        let latest_block_bytes = latest_block.to_be_bytes();
        let items = vec![(LATEST_BLOCK_KEY, &latest_block_bytes)];

        self.batch_put(items).await;
    }
}
