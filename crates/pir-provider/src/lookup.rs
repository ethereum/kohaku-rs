use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use crate::PirProviderError;

/// Blocking key lookup used by the router (wrapped in `spawn_blocking`).
pub trait LookupBackend: Send + Sync {
    /// Return the raw value bytes for `key`, or `None` if the key is absent.
    ///
    /// # Errors
    ///
    /// Returns [`PirProviderError::Client`] when the PIR client fails.
    fn lookup(&self, key: &[u8]) -> Result<Option<Vec<u8>>, PirProviderError>;
}

/// `pir-client` backend. `PirClient::lookup` needs `&mut self`, so this is
/// mutex-wrapped and must run off the async runtime.
#[cfg(feature = "client")]
pub struct PirLookup {
    client: Mutex<pir_client::PirClient>,
}

#[cfg(feature = "client")]
impl PirLookup {
    /// Wrap an already-connected client.
    #[must_use]
    pub fn new(client: pir_client::PirClient) -> Self {
        Self {
            client: Mutex::new(client),
        }
    }
}

#[cfg(feature = "client")]
impl LookupBackend for PirLookup {
    fn lookup(&self, key: &[u8]) -> Result<Option<Vec<u8>>, PirProviderError> {
        let mut client = self.client.lock().unwrap_or_else(PoisonError::into_inner);
        client
            .lookup(key)
            .map(|found| found.map(|lookup| lookup.value))
            .map_err(PirProviderError::Client)
    }
}

/// In-memory lookup for tests.
#[derive(Default)]
pub struct MapLookup {
    /// Raw PIR key → value bytes.
    pub entries: Mutex<HashMap<Vec<u8>, Vec<u8>>>,
}

impl MapLookup {
    /// Insert a key/value pair.
    pub fn insert(&self, key: impl AsRef<[u8]>, value: impl AsRef<[u8]>) {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key.as_ref().to_vec(), value.as_ref().to_vec());
    }
}

impl LookupBackend for MapLookup {
    fn lookup(&self, key: &[u8]) -> Result<Option<Vec<u8>>, PirProviderError> {
        Ok(self
            .entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(key)
            .cloned())
    }
}
