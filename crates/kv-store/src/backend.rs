/// Generic key-value store interface.
#[async_trait::async_trait]
pub trait KvStoreBackend: Send + Sync {
    /// See [`crate::Store::get_batch`].
    async fn get_batch(&self, keys: &[&[u8]]) -> Vec<Option<Vec<u8>>>;

    /// See [`crate::Store::put_batch`].
    async fn put_batch(&self, items: &[(&[u8], &[u8])]);

    /// See [`crate::Store::delete_batch`].
    async fn delete_batch(&self, keys: &[&[u8]]);
}
