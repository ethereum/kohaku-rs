use thiserror::Error;
use tracing::info;

use self::{
    decode::{parse_chunk_events, verify_and_decompress_chunk},
    manifest::Manifest,
};
use crate::{
    indexer::syncer::{SyncEvent, SyncerBackend, SyncerError},
    provider::pool::Pool,
};

mod decode;
mod manifest;

/// A syncer that reads pre-scraped, verifiable event chunks published under the
/// [saga-sync](https://github.com/fatlabsxyz/saga-sync) protocol.
///
/// Fetches the protocol's manifest, verifies each overlapping chunk's sha256 digest against it,
/// and decodes the chunk's events into [`SyncEvent`]s.
pub struct SagaSyncSyncer {
    client: reqwest::Client,
    base_url: String,
}

#[derive(Debug, Error)]
pub enum SagaSyncError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Hex decode error: {0}")]
    Hex(#[from] hex::FromHexError),
    #[error("Gzip decode error: {0}")]
    Gzip(#[from] std::io::Error),
    #[error("Chunk is not valid UTF-8: {0}")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("Event decode error: {0}")]
    EventDecode(#[from] alloy::sol_types::Error),
    #[error("Chunk digest mismatch: expected {expected}, got {actual}")]
    DigestMismatch { expected: String, actual: String },
    #[error("Chunk events out of order at line {index}")]
    OutOfOrder { index: usize },
    #[error("Event at block {block} outside chunk range [{from}, {to})")]
    OutOfRange { block: u64, from: u64, to: u64 },
}

impl SagaSyncSyncer {
    #[must_use]
    pub fn new(base_url: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }
}

#[async_trait::async_trait]
impl SyncerBackend for SagaSyncSyncer {
    async fn latest_block(&self, pool: &Pool) -> Result<u64, SyncerError> {
        let manifest = self.fetch_manifest().await.map_err(SyncerError::other)?;
        let key = stream_key(pool);

        Ok(manifest
            .streams
            .get(&key)
            .and_then(manifest::StreamEntry::last_block)
            .unwrap_or(pool.deployed_block))
    }

    async fn sync(
        &self,
        pool: &Pool,
        from_block: u64,
        to_block: u64,
    ) -> Result<Vec<SyncEvent>, SyncerError> {
        info!("Syncing from {} to {}", from_block, to_block);

        let manifest = self.fetch_manifest().await.map_err(SyncerError::other)?;
        let key = stream_key(pool);
        let Some(entry) = manifest.streams.get(&key) else {
            return Ok(Vec::new());
        };

        let overlapping = entry
            .chunks
            .iter()
            .chain(entry.hot_head.iter())
            .filter(|c| c.from_block < to_block && c.to_block > from_block);

        let mut all_events = Vec::new();
        for chunk in overlapping {
            let gz_bytes = self
                .fetch_chunk_bytes(&chunk.file)
                .await
                .map_err(SyncerError::other)?;

            let decompressed =
                verify_and_decompress_chunk(&gz_bytes, chunk).map_err(SyncerError::other)?;

            let events = parse_chunk_events(&decompressed, chunk, from_block..to_block)
                .map_err(SyncerError::other)?;

            all_events.extend(events);
        }

        Ok(all_events)
    }
}

impl SagaSyncSyncer {
    async fn fetch_manifest(&self) -> Result<Manifest, SagaSyncError> {
        let bytes = self
            .client
            .get(self.manifest_url())
            .send()
            .await?
            .bytes()
            .await?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    async fn fetch_chunk_bytes(&self, file: &str) -> Result<Vec<u8>, SagaSyncError> {
        let bytes = self
            .client
            .get(self.chunk_url(file))
            .send()
            .await?
            .bytes()
            .await?;
        Ok(bytes.to_vec())
    }

    fn manifest_url(&self) -> String {
        format!("{}/index.json", self.base_url)
    }

    fn chunk_url(&self, file: &str) -> String {
        format!("{}/{}", self.base_url, file)
    }
}

#[must_use]
fn stream_key(pool: &Pool) -> String {
    format!(
        "tornado-cash-{}-{}-{}",
        pool.chain_id,
        pool.symbol().to_lowercase(),
        pool.amount()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_stream_key() {
        assert_eq!(
            stream_key(&Pool::ETHEREUM_ETHER_01),
            "tornado-cash-1-eth-0.1"
        );
    }
}
