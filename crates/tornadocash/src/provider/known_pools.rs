use kohaku_kv_store::Store;
use tracing::warn;

use crate::pool::Pool;

const KNOWN_POOLS_KEY: &[u8] = b"known_pools";

/// Loads the set of pools this provider has previously seen, if any.
///
/// A missing key or a decode failure is treated as "no known pools yet" rather than an error,
/// since losing this list only degrades the "remembered across restarts" convenience.
pub(super) async fn load(store: &Store) -> Vec<Pool> {
    let Ok(Some(bytes)) = store.get(KNOWN_POOLS_KEY).await else {
        return Vec::new();
    };

    match postcard::from_bytes(&bytes) {
        Ok(pools) => pools,
        Err(err) => {
            warn!("failed to decode known pools: {err}");
            Vec::new()
        }
    }
}

/// Persists the set of pools this provider has seen. Best-effort: failures are logged, not
/// propagated, since losing this list only degrades the "remembered across restarts"
/// convenience rather than the operation that triggered the persist.
pub(super) async fn save(store: &Store, pools: &[Pool]) {
    let bytes = match postcard::to_allocvec(pools) {
        Ok(bytes) => bytes,
        Err(err) => {
            warn!("failed to encode known pools: {err}");
            return;
        }
    };

    if let Err(err) = store.put(KNOWN_POOLS_KEY, bytes).await {
        warn!("failed to persist known pools: {err}");
    }
}
