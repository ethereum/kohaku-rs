use std::{path::Path, sync::Arc};

use redb::{Database, ReadableDatabase, TableDefinition, TableError};

use crate::backend::KvStoreBackend;

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
    async fn get_batch(&self, keys: &[&[u8]]) -> Vec<Option<Vec<u8>>> {
        let db = self.db.clone();
        let keys: Vec<Vec<u8>> = keys.iter().map(|key| key.to_vec()).collect();
        tokio::task::spawn_blocking(move || {
            let read_txn = db.begin_read().expect("failed to begin read transaction");
            let table = match read_txn.open_table(TABLE) {
                Ok(table) => Some(table),
                Err(TableError::TableDoesNotExist(_)) => None,
                Err(err) => panic!("failed to open table: {err}"),
            };
            keys.iter()
                .map(|key| {
                    let value = table
                        .as_ref()?
                        .get(key.as_slice())
                        .expect("failed to read key");
                    value.map(|guard| guard.value().to_vec())
                })
                .collect()
        })
        .await
        .expect("get_batch task panicked")
    }

    async fn put_batch(&self, items: &[(&[u8], &[u8])]) {
        let db = self.db.clone();
        let items: Vec<(Vec<u8>, Vec<u8>)> = items
            .iter()
            .map(|(key, value)| (key.to_vec(), value.to_vec()))
            .collect();
        tokio::task::spawn_blocking(move || {
            let write_txn = db.begin_write().expect("failed to begin write transaction");
            {
                let mut table = write_txn.open_table(TABLE).expect("failed to open table");
                for (key, value) in &items {
                    table
                        .insert(key.as_slice(), value.as_slice())
                        .expect("failed to insert key");
                }
            }
            write_txn
                .commit()
                .expect("failed to commit write transaction");
        })
        .await
        .expect("put_batch task panicked");
    }

    async fn delete_batch(&self, keys: &[&[u8]]) {
        let db = self.db.clone();
        let keys: Vec<Vec<u8>> = keys.iter().map(|key| key.to_vec()).collect();
        tokio::task::spawn_blocking(move || {
            let write_txn = db.begin_write().expect("failed to begin write transaction");
            {
                let mut table = write_txn.open_table(TABLE).expect("failed to open table");
                for key in &keys {
                    table.remove(key.as_slice()).expect("failed to remove key");
                }
            }
            write_txn
                .commit()
                .expect("failed to commit write transaction");
        })
        .await
        .expect("delete_batch task panicked");
    }
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
            .await;

        assert_eq!(
            store.get_batch(["key1", "key2"]).await,
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
            .await;
        store.delete_batch(["key1", "key2"]).await;

        assert_eq!(
            store.get_batch(["key1", "key2", "key3"]).await,
            vec![None, None, Some(b"value3".to_vec())]
        );
    }

    #[tokio::test]
    async fn overwriting_a_value_updates_it() {
        let (store, _dir) = store();
        store.put("key", "value1").await;
        store.put("key", "value2").await;

        assert_eq!(store.get("key").await, Some(b"value2".to_vec()));
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
        store.put("key", "value").await;
        drop(store);

        let store: Store = FileStore::open(&path).expect("failed to open store").into();
        assert_eq!(store.get("key").await, Some(b"value".to_vec()));
    }
}
