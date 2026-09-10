#![doc = include_str!("../README.md")]

use std::sync::Arc;

use crate::backend::KvStoreBackend;

pub mod backend;
pub mod memory;

/// Separates a scope's prefix from the next scope segment or the leaf key.
///
/// Scope names and keys must not contain this byte, or distinct
/// scopes/keys can collide on the same underlying storage key.
const SCOPE_DELIMITER: u8 = 0;

/// A generic key-value store.
///
/// This is a thin wrapper around a [`KvStoreBackend`] implementation.
#[derive(Clone)]
pub struct Store {
    backend: Arc<dyn KvStoreBackend>,
    prefix: Vec<u8>,
}

impl Store {
    pub fn new(backend: impl KvStoreBackend + 'static) -> Self {
        Self {
            backend: Arc::new(backend),
            prefix: Vec::new(),
        }
    }

    /// Returns a [`Store`] narrowed to the given namespace, sharing the same
    /// underlying backend.
    ///
    /// Keys written through the returned store are prefixed so they cannot
    /// collide with keys in sibling scopes or the parent scope.
    pub fn scope(&self, name: impl AsRef<[u8]>) -> Self {
        let mut prefix = self.prefix.clone();
        prefix.extend_from_slice(name.as_ref());
        prefix.push(SCOPE_DELIMITER);
        Self {
            backend: self.backend.clone(),
            prefix,
        }
    }

    /// Gets the value associated with the given key.
    pub async fn get(&self, key: impl AsRef<[u8]>) -> Option<Vec<u8>> {
        self.backend.get(&self.scoped_key(key)).await
    }

    /// Puts a value associated with the given key.
    pub async fn put(&self, key: impl AsRef<[u8]>, value: impl AsRef<[u8]>) {
        self.backend
            .put(&self.scoped_key(key), value.as_ref())
            .await;
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
        let owned: Vec<(Vec<u8>, V)> = items
            .into_iter()
            .map(|(k, v)| (self.scoped_key(k), v))
            .collect();
        let refs: Vec<(&[u8], &[u8])> = owned
            .iter()
            .map(|(k, v)| (k.as_slice(), v.as_ref()))
            .collect();
        self.backend.batch_put(&refs).await;
    }

    /// Deletes the value associated with the given key.
    pub async fn delete(&self, key: impl AsRef<[u8]>) {
        self.backend.delete(&self.scoped_key(key)).await;
    }

    fn scoped_key(&self, key: impl AsRef<[u8]>) -> Vec<u8> {
        let mut scoped = self.prefix.clone();
        scoped.extend_from_slice(key.as_ref());
        scoped
    }
}

impl<T: KvStoreBackend + 'static> From<T> for Store {
    fn from(backend: T) -> Self {
        Self::new(backend)
    }
}
