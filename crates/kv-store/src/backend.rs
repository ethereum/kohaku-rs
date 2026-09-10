/// Generic key-value store interface.
#[async_trait::async_trait]
pub trait KvStoreBackend: Send + Sync {
    /// See [`Store::get`].
    async fn get(&self, key: &[u8]) -> Option<Vec<u8>>;

    /// See [`Store::put`].
    async fn put(&self, key: &[u8], value: &[u8]) {
        self.batch_put(&[(key, value)]).await;
    }

    /// See [`Store::batch_put`].
    async fn batch_put(&self, items: &[(&[u8], &[u8])]);

    /// See [`Store::delete`].
    async fn delete(&self, key: &[u8]);
}
