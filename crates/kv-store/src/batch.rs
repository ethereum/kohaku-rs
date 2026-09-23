use crate::{Store, backend::StoreError};

pub struct Batch<'a> {
    store: &'a Store,
    items: Vec<(Vec<u8>, Vec<u8>)>,
}

impl<'a> Batch<'a> {
    pub fn new(store: &'a Store) -> Self {
        Self {
            store,
            items: Vec::new(),
        }
    }

    /// Adds a key-value pair to the batch.
    pub fn put(&mut self, key: impl AsRef<[u8]>, value: impl AsRef<[u8]>) -> &mut Self {
        self.items
            .push((key.as_ref().to_vec(), value.as_ref().to_vec()));
        self
    }

    /// Commits the batch to the underlying store.
    pub async fn commit(self) -> Result<(), StoreError> {
        self.store.put_batch(self.items).await
    }
}
