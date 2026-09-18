use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard},
};

use crate::backend::{KvStoreBackend, StoreError};

/// An in-memory [`KvStoreBackend`], useful for tests and other non-persistent use cases.
#[derive(Default)]
pub struct MemoryStore(Mutex<HashMap<Vec<u8>, Vec<u8>>>);

impl MemoryStore {
    pub fn new() -> Self {
        Self(Mutex::new(HashMap::new()))
    }
}

#[async_trait::async_trait]
impl KvStoreBackend for MemoryStore {
    async fn get_batch(&self, keys: &[&[u8]]) -> Result<Vec<Option<Vec<u8>>>, StoreError> {
        let store = self.lock();
        Ok(keys.iter().map(|&key| store.get(key).cloned()).collect())
    }

    async fn put_batch(&self, items: &[(&[u8], &[u8])]) -> Result<(), StoreError> {
        let mut store = self.lock();
        for &(key, value) in items {
            store.insert(key.to_vec(), value.to_vec());
        }

        Ok(())
    }

    async fn delete_batch(&self, keys: &[&[u8]]) -> Result<(), StoreError> {
        let mut store = self.lock();
        for &key in keys {
            store.remove(key);
        }

        Ok(())
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

    fn store() -> Store {
        MemoryStore::new().into()
    }

    #[tokio::test]
    async fn put_and_get_multiple_values() {
        let store = store();
        store
            .put_batch(vec![("key1", "value1"), ("key2", "value2")])
            .await
            .unwrap();

        assert_eq!(
            store.get_batch(["key1", "key2"]).await.unwrap(),
            vec![Some(b"value1".to_vec()), Some(b"value2".to_vec())]
        );
    }

    #[tokio::test]
    async fn delete_multiple_values() {
        let store = store();
        store
            .put_batch(vec![
                ("key1", "value1"),
                ("key2", "value2"),
                ("key3", "value3"),
            ])
            .await
            .unwrap();
        store.delete_batch(["key1", "key2"]).await.unwrap();

        assert_eq!(
            store.get_batch(["key1", "key2", "key3"]).await.unwrap(),
            vec![None, None, Some(b"value3".to_vec())]
        );
    }

    #[tokio::test]
    async fn overwriting_a_value_updates_it() {
        let store = store();
        store.put("key", "value1").await.unwrap();
        store.put("key", "value2").await.unwrap();

        assert_eq!(store.get("key").await.unwrap(), Some(b"value2".to_vec()));
    }
}
