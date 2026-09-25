use kohaku_kv_store::{Store, backend::StoreError};

const NEXT_NONCE_KEY: &[u8] = b"next_nonce";

#[async_trait::async_trait]
pub trait WalletStoreExt {
    async fn next_nonce(&self) -> Result<u64, StoreError>;
    async fn set_next_nonce(&self, nonce: u64) -> Result<(), StoreError>;
}

#[async_trait::async_trait]
impl WalletStoreExt for Store {
    async fn next_nonce(&self) -> Result<u64, StoreError> {
        Ok(self.get(NEXT_NONCE_KEY).await?.map_or(0, |v| {
            u64::from_be_bytes(v.try_into().expect("next nonce is 8 bytes"))
        }))
    }

    async fn set_next_nonce(&self, nonce: u64) -> Result<(), StoreError> {
        self.put(NEXT_NONCE_KEY, nonce.to_be_bytes()).await
    }
}
