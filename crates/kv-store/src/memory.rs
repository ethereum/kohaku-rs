use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard},
};

use crate::backend::KvStoreBackend;

/// An in-memory [`KvStore`], useful for tests and other non-persistent use cases.
#[derive(Debug, Default)]
pub struct MemoryStore(Mutex<HashMap<Vec<u8>, Vec<u8>>>);

impl MemoryStore {
    pub fn new() -> Self {
        Self(Mutex::new(HashMap::new()))
    }
}

#[async_trait::async_trait]
impl KvStoreBackend for MemoryStore {
    async fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.lock().get(key).cloned()
    }

    async fn batch_put(&self, items: &[(&[u8], &[u8])]) {
        let mut store = self.lock();
        for &(key, value) in items {
            store.insert(key.to_vec(), value.to_vec());
        }
    }

    async fn delete(&self, key: &[u8]) {
        self.lock().remove(key);
    }
}

impl MemoryStore {
    fn lock(&self) -> MutexGuard<'_, HashMap<Vec<u8>, Vec<u8>>> {
        #[expect(
            clippy::unwrap_used,
            reason = "Mutex cannot be poisoned in this context"
        )]
        self.0.lock().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;

    /// Asserts that MemoryStore returns None for a nonexistent key.
    #[tokio::test]
    async fn test_memory_store_get_none() {
        let store: Store = MemoryStore::new().into();
        assert_eq!(store.get("nonexistent_key").await, None);
    }

    /// Asserts that MemoryStore can put and get a value correctly.
    #[tokio::test]
    async fn test_memory_store_get_put() {
        let store: Store = MemoryStore::new().into();

        store.put("key", "value").await;
        assert_eq!(store.get("key").await, Some(b"value".to_vec()));
    }

    /// Asserts that MemoryStore returns None for a deleted key.
    #[tokio::test]
    async fn test_memory_store_get_deleted() {
        let store: Store = MemoryStore::new().into();

        store.put("key", "value").await;
        store.delete("key").await;

        assert_eq!(store.get("key").await, None);
    }

    /// Asserts that overwriting a key in MemoryStore updates the value correctly.
    #[tokio::test]
    async fn test_memory_store_overwrite() {
        let store: Store = MemoryStore::new().into();

        store.put("key", "test_value_1").await;
        assert_eq!(store.get("key").await, Some(b"test_value_1".to_vec()));

        store.put("key", "test_value_2").await;
        assert_eq!(store.get("key").await, Some(b"test_value_2".to_vec()));
    }

    /// Asserts that putting an unrelated key in MemoryStore does not affect other keys.
    #[tokio::test]
    async fn test_memory_store_unrelated_key() {
        let store: Store = MemoryStore::new().into();

        store.put("key1", "value1").await;
        assert_eq!(store.get("key1").await, Some(b"value1".to_vec()));

        store.put("key2", "value2").await;
        assert_eq!(store.get("key1").await, Some(b"value1".to_vec()));
        assert_eq!(store.get("key2").await, Some(b"value2".to_vec()));
    }
}
