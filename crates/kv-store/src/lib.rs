use std::sync::Arc;

use crate::backend::KvStoreBackend;

pub mod backend;
pub mod memory;

/// A generic key-value store.
///
/// This is a thin wrapper around a [`KvStoreBackend`] implementation.
#[derive(Clone)]
pub struct Store(Arc<dyn KvStoreBackend>);

impl Store {
    pub fn new(backend: impl KvStoreBackend + 'static) -> Self {
        Self(Arc::new(backend))
    }

    /// Gets the value associated with the given key.
    pub async fn get(&self, key: impl AsRef<[u8]>) -> Option<Vec<u8>> {
        self.0.get(key.as_ref()).await
    }

    /// Puts a value associated with the given key.
    pub async fn put(&self, key: impl AsRef<[u8]>, value: impl AsRef<[u8]>) {
        self.0.put(key.as_ref(), value.as_ref()).await;
    }

    /// Puts multiple key-value pairs in a batch operation.
    ///
    /// The batch operation must be atomic, meaning either all kv pairs are
    /// written or none are written. Rollbacks should automatically occur if any
    /// part of the batch operation fails.
    pub async fn batch_put<K, V>(&self, items: impl IntoIterator<Item = (K, V)>)
    where
        K: AsRef<[u8]>,
        V: AsRef<[u8]>,
    {
        let owned: Vec<(K, V)> = items.into_iter().collect();
        let refs: Vec<(&[u8], &[u8])> = owned
            .iter()
            .map(|(k, v)| (k.as_ref(), v.as_ref()))
            .collect();
        self.0.batch_put(&refs).await;
    }

    /// Deletes the value associated with the given key.
    pub async fn delete(&self, key: impl AsRef<[u8]>) {
        self.0.delete(key.as_ref()).await;
    }
}

impl<T: KvStoreBackend + 'static> From<T> for Store {
    fn from(backend: T) -> Self {
        Self::new(backend)
    }
}
