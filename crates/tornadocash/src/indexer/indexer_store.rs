use kohaku_kv_store::{Store, backend::StoreError};

const LATEST_BLOCK_KEY: &[u8] = b"latest_block";

#[async_trait::async_trait]
pub trait IndexerStoreExt {
    async fn latest_block(&self) -> Result<u64, StoreError>;
    async fn commit(&self, latest_block: u64) -> Result<(), StoreError>;
}

#[async_trait::async_trait]
impl IndexerStoreExt for Store {
    async fn latest_block(&self) -> Result<u64, StoreError> {
        Ok(self.get(LATEST_BLOCK_KEY).await?.map_or(0, |v| {
            u64::from_be_bytes(v.try_into().expect("latest block is 8 bytes"))
        }))
    }

    /// Atomically writes `latest_block` to the store.
    async fn commit(&self, latest_block: u64) -> Result<(), StoreError> {
        let latest_block_bytes = latest_block.to_be_bytes();
        let items = vec![(LATEST_BLOCK_KEY, &latest_block_bytes)];

        self.put_batch(items).await
    }
}
