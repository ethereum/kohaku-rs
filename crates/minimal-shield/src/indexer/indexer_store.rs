use kohaku_kv_store::Store;

const LATEST_BLOCK_KEY: &[u8] = b"latest_block";
const EPOCH_KEY: &[u8] = b"epoch";

#[async_trait::async_trait]
pub trait IndexerStoreExt {
    async fn latest_block(&self) -> u64;
    async fn epoch(&self) -> u64;
    async fn commit(&self, latest_block: u64, epoch: u64);
}

#[async_trait::async_trait]
impl IndexerStoreExt for Store {
    async fn latest_block(&self) -> u64 {
        self.get(LATEST_BLOCK_KEY)
            .await
            .ok()
            .flatten()
            .and_then(|v| v.try_into().ok())
            .map(u64::from_be_bytes)
            .unwrap_or(0)
    }

    async fn epoch(&self) -> u64 {
        self.get(EPOCH_KEY)
            .await
            .ok()
            .flatten()
            .and_then(|v| v.try_into().ok())
            .map(u64::from_be_bytes)
            .unwrap_or(0)
    }

    async fn commit(&self, latest_block: u64, epoch: u64) {
        let lb = latest_block.to_be_bytes();
        let ep = epoch.to_be_bytes();
        self.put_batch(vec![
            (LATEST_BLOCK_KEY, lb.as_slice()),
            (EPOCH_KEY, ep.as_slice()),
        ])
        .await
        .expect("indexer commit");
    }
}
