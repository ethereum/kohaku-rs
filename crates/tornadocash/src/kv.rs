/// Generic key-value store interface.  Abstracts over different storage backends selected by SDK
/// consumers.
#[async_trait::async_trait]
pub trait KvStore: Send + Sync {
    /// Get the value associated with the given key.
    async fn get(&self, key: &[u8]) -> Option<Vec<u8>>;

    /// Puts the value associated with the given key.
    async fn put(&self, key: &[u8], value: &[u8]) {
        self.batch_put(&[(key, value)]).await;
    }

    /// Puts multiple key-value pairs in a batch operation.
    ///
    /// The batch operation must be atomic, meaning either all kv pairs are
    /// written or none are written.
    async fn batch_put(&self, items: &[(&[u8], &[u8])]);

    /// Deletes the value associated with the given key.
    async fn delete(&self, key: &[u8]);
}

/// An in-memory `KvStore`, useful for tests and other non-persistent use cases.
#[derive(Debug, Default)]
pub struct MemoryKvStore(std::sync::Mutex<std::collections::HashMap<Vec<u8>, Vec<u8>>>);

#[async_trait::async_trait]
impl KvStore for MemoryKvStore {
    async fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.0.lock().unwrap().get(key).cloned()
    }

    async fn batch_put(&self, items: &[(&[u8], &[u8])]) {
        let mut store = self.0.lock().unwrap();
        for &(key, value) in items {
            store.insert(key.to_vec(), value.to_vec());
        }
    }

    async fn delete(&self, key: &[u8]) {
        self.0.lock().unwrap().remove(key);
    }
}
