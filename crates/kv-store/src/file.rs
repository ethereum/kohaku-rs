use std::{path::Path, sync::Arc};

use redb::{Database, ReadableDatabase, TableDefinition, TableError};

use crate::backend::{KvStoreBackend, StoreError};

/// A persistent, disk-backed [`KvStoreBackend`] with ACID transactions.
///
/// Backed by a single [`redb`](https://docs.rs/redb/latest/redb/) table.
pub struct FileStore {
    db: Arc<Database>,
}

#[derive(Debug, thiserror::Error)]
pub enum FileStoreError {
    #[error("failed to open database file: {0}")]
    Open(#[from] redb::DatabaseError),
}

const TABLE: TableDefinition<'static, &[u8], &[u8]> = TableDefinition::new("kv");

impl FileStore {
    /// Opens the redb database file at `path`, creating it if it does not exist.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, FileStoreError> {
        let db = Database::create(path)?;
        Ok(Self { db: Arc::new(db) })
    }
}

#[async_trait::async_trait]
impl KvStoreBackend for FileStore {
    async fn get_batch(&self, keys: &[&[u8]]) -> Result<Vec<Option<Vec<u8>>>, StoreError> {
        let db = self.db.clone();
        let keys: Vec<Vec<u8>> = keys.iter().map(|key| key.to_vec()).collect();
        tokio::task::spawn_blocking(move || {
            let read_txn = db.begin_read().map_err(store_error)?;
            let table = match read_txn.open_table(TABLE) {
                Ok(table) => Some(table),
                Err(TableError::TableDoesNotExist(_)) => None,
                Err(err) => return Err(store_error(err)),
            };
            keys.iter()
                .map(|key| {
                    let Some(table) = table.as_ref() else {
                        return Ok(None);
                    };
                    let value = table.get(key.as_slice()).map_err(store_error)?;
                    Ok(value.map(|guard| guard.value().to_vec()))
                })
                .collect()
        })
        .await
        .expect("get_batch task panicked")
    }

    async fn put_batch(&self, items: &[(&[u8], &[u8])]) -> Result<(), StoreError> {
        let db = self.db.clone();
        let items: Vec<(Vec<u8>, Vec<u8>)> = items
            .iter()
            .map(|(key, value)| (key.to_vec(), value.to_vec()))
            .collect();
        tokio::task::spawn_blocking(move || {
            let write_txn = db.begin_write().map_err(store_error)?;
            {
                let mut table = write_txn.open_table(TABLE).map_err(store_error)?;
                for (key, value) in &items {
                    table
                        .insert(key.as_slice(), value.as_slice())
                        .map_err(store_error)?;
                }
            }
            write_txn.commit().map_err(store_error)
        })
        .await
        .expect("put_batch task panicked")
    }

    async fn delete_batch(&self, keys: &[&[u8]]) -> Result<(), StoreError> {
        let db = self.db.clone();
        let keys: Vec<Vec<u8>> = keys.iter().map(|key| key.to_vec()).collect();
        tokio::task::spawn_blocking(move || {
            let write_txn = db.begin_write().map_err(store_error)?;
            {
                let mut table = write_txn.open_table(TABLE).map_err(store_error)?;
                for key in &keys {
                    table.remove(key.as_slice()).map_err(store_error)?;
                }
            }
            write_txn.commit().map_err(store_error)
        })
        .await
        .expect("delete_batch task panicked")
    }
}

/// Boxes a redb error into a [`StoreError`].
fn store_error(err: impl std::error::Error + Send + Sync + 'static) -> StoreError {
    StoreError::from(Box::new(err) as Box<dyn std::error::Error + Send + Sync>)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;

    fn store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let store: Store = FileStore::open(dir.path().join("test.redb"))
            .expect("failed to open store")
            .into();
        (store, dir)
    }

    #[tokio::test]
    async fn put_and_get_multiple_values() {
        let (store, _dir) = store();
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
        let (store, _dir) = store();
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
        let (store, _dir) = store();
        store.put("key", "value1").await.unwrap();
        store.put("key", "value2").await.unwrap();

        assert_eq!(store.get("key").await.unwrap(), Some(b"value2".to_vec()));
    }

    #[tokio::test]
    async fn open_creates_missing_file() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let path = dir.path().join("new.redb");

        assert!(!path.exists());
        FileStore::open(&path).expect("failed to open store");
        assert!(path.exists());
    }

    #[tokio::test]
    async fn data_survives_reopen() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let path = dir.path().join("test.redb");

        let store: Store = FileStore::open(&path).expect("failed to open store").into();
        store.put("key", "value").await.unwrap();
        drop(store);

        let store: Store = FileStore::open(&path).expect("failed to open store").into();
        assert_eq!(store.get("key").await.unwrap(), Some(b"value".to_vec()));
    }
}
