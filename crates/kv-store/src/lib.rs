#![doc = include_str!("../README.md")]

use std::sync::Arc;

use crate::{
    backend::{KvStoreBackend, StoreError},
    batch::Batch,
};

pub mod backend;
pub mod batch;
pub mod file;
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

    /// Creates a new store with an in-memory backend.
    pub fn create() -> Self {
        Self::new(memory::MemoryStore::new())
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

    pub fn batch(&self) -> Batch<'_> {
        Batch::new(self)
    }

    /// Gets the value associated with the given key.
    pub async fn get(&self, key: impl AsRef<[u8]>) -> Result<Option<Vec<u8>>, StoreError> {
        Ok(self
            .backend
            .get_batch(&[&self.scoped_key(key)])
            .await?
            .into_iter()
            .next()
            .unwrap_or(None))
    }

    /// Gets the values associated with the given keys in a batch operation.
    ///
    /// Will always return a vector of the same length as the input keys, with `None` for keys that
    /// do not exist in the store.
    pub async fn get_batch<K>(
        &self,
        keys: impl IntoIterator<Item = K>,
    ) -> Result<Vec<Option<Vec<u8>>>, StoreError>
    where
        K: AsRef<[u8]>,
    {
        let scoped_keys: Vec<Vec<u8>> = keys.into_iter().map(|k| self.scoped_key(k)).collect();
        let key_refs: Vec<&[u8]> = scoped_keys.iter().map(|k| k.as_slice()).collect();
        self.backend.get_batch(&key_refs).await
    }

    /// Puts a value associated with the given key.
    pub async fn put(
        &self,
        key: impl AsRef<[u8]>,
        value: impl AsRef<[u8]>,
    ) -> Result<(), StoreError> {
        self.backend
            .put_batch(&[(&self.scoped_key(key), value.as_ref())])
            .await
    }

    /// Puts multiple key-value pairs in a batch operation.
    ///
    /// The batch operation must be atomic, meaning either all kv pairs are
    /// written or none are written. Rollbacks should automatically occur if any
    /// part of the batch operation fails.
    pub async fn put_batch<K, V>(
        &self,
        items: impl IntoIterator<Item = (K, V)>,
    ) -> Result<(), StoreError>
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
        self.backend.put_batch(&refs).await
    }

    /// Deletes the value associated with the given key.
    pub async fn delete(&self, key: impl AsRef<[u8]>) -> Result<(), StoreError> {
        self.backend.delete_batch(&[&self.scoped_key(key)]).await
    }

    /// Deletes multiple keys in a batch operation.
    ///
    /// The batch operation must be atomic, meaning either all keys are deleted or none are deleted.
    /// Rollbacks should automatically occur if any part of the batch operation fails.
    pub async fn delete_batch<K>(&self, keys: impl IntoIterator<Item = K>) -> Result<(), StoreError>
    where
        K: AsRef<[u8]>,
    {
        let scoped_keys: Vec<Vec<u8>> = keys.into_iter().map(|k| self.scoped_key(k)).collect();
        let key_refs: Vec<&[u8]> = scoped_keys.iter().map(|k| k.as_slice()).collect();
        self.backend.delete_batch(&key_refs).await
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
