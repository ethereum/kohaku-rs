use kohaku_tornadocash::{Pool, event::Snapshot};
use tracing::info;

use self::{
    decode::{parse_chunk_events, verify_and_decompress_chunk},
    manifest::Manifest,
};
use crate::{Syncer, SyncerError};

mod decode;
mod manifest;

/// A syncer that reads pre-scraped event chunks published under the
/// [saga-sync](https://github.com/fatlabsxyz/saga-sync) protocol.
///
/// Fetches the protocol's manifest, verifies each overlapping chunk's sha256 digest against it,
/// and decodes the chunk's events into [`SyncEvent`](kohaku_tornadocash::event::SyncEvent)s.
///
/// Does not currently implement chunk caching or chunk signature verification.
pub struct SagaSyncer {
    client: reqwest::Client,
    base_url: String,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
enum SagaSyncError {
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
    #[error("Field conversion error: {0}")]
    Field(#[from] kohaku_tornadocash::FieldNotInRangeError),
    #[error("Snapshot error: {0}")]
    Snapshot(#[from] kohaku_tornadocash::event::SnapshotError),
    #[error("Missing manifest entry for pool {0}")]
    MissingManifestEntry(Pool),
}

impl SagaSyncer {
    #[must_use]
    pub fn new(base_url: &str) -> Self {
        Self::with_client(reqwest::Client::new(), base_url)
    }

    /// `client` is the HTTP client for the manifest and chunk fetches.
    ///
    /// A later caller can pass a client that uses a proxy without this syncer knowing why.
    #[must_use]
    pub fn with_client(client: reqwest::Client, base_url: &str) -> Self {
        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }
}

#[cfg_attr(native, async_trait::async_trait)]
#[cfg_attr(wasm, async_trait::async_trait(?Send))]
impl Syncer for SagaSyncer {
    async fn sync_range(
        &self,
        pool: &Pool,
        from_block: u64,
        to_block: u64,
    ) -> Result<Snapshot, SyncerError> {
        self.sync(pool, from_block, to_block)
            .await
            .map_err(SyncerError::other)
    }
}

impl SagaSyncer {
    async fn sync(
        &self,
        pool: &Pool,
        from_block: u64,
        to_block: u64,
    ) -> Result<Snapshot, SagaSyncError> {
        let manifest = self.fetch_manifest().await?;
        let key = stream_key(pool);

        let from = from_block.max(pool.deployed_block);
        let Some(entry) = manifest.available_protocols.get(&key) else {
            return Err(SagaSyncError::MissingManifestEntry(pool.clone()));
        };

        //? Chunk bounds are already half-open, so the last chunk's `to_block` is the exclusive
        //? end of everything this stream publishes.
        let published = entry.last_block().unwrap_or(pool.deployed_block);
        let range = from..to_block.min(published).max(from);

        info!("Syncing from {} to {}", range.start, range.end);

        let overlapping = entry
            .chunks
            .iter()
            .chain(entry.hot_head.iter())
            .filter(|c| c.from_block < range.end && c.to_block > range.start);

        let mut events = Vec::new();
        for chunk in overlapping {
            let gz_bytes = self.fetch_chunk_bytes(&chunk.file).await?;

            let decompressed = verify_and_decompress_chunk(&gz_bytes, chunk)?;

            let decoded = parse_chunk_events(&decompressed, chunk, range.clone())?;

            events.extend(decoded);
        }

        Ok(Snapshot::new(pool.clone(), range, events)?)
    }

    async fn fetch_manifest(&self) -> Result<Manifest, SagaSyncError> {
        let bytes = self
            .client
            .get(self.manifest_url())
            .send()
            .await?
            .error_for_status()?
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
            .error_for_status()?
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
        pool.symbol(),
        pool.amount()
    )
}

#[cfg(test)]
mod tests {
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        thread,
    };

    use alloy::primitives::B256;
    use reqwest::StatusCode;

    use super::*;

    const HOT_HEAD_FILE: &str = "tornado-cash-11155111-eth-0.1.hot.jsonl.gz";
    //? The body S3 sends for a missing key.
    const NO_SUCH_KEY: &str =
        "<?xml version='1.0' encoding='UTF-8'?><Error><Code>NoSuchKey</Code></Error>";

    #[test]
    fn derives_stream_key() {
        assert_eq!(
            stream_key(&Pool::ETHEREUM_ETHER_01),
            "tornado-cash-1-eth-0.1"
        );
    }

    #[tokio::test]
    async fn reports_a_missing_manifest_as_an_http_error() {
        let base_url = serve(vec![(
            "/index.json".into(),
            StatusCode::NOT_FOUND,
            NO_SUCH_KEY.into(),
        )]);

        let err = loopback_syncer(&base_url)
            .sync(&Pool::SEPOLIA_ETHER_01, 0, u64::MAX)
            .await
            .unwrap_err();

        assert_eq!(http_status(&err), Some(StatusCode::NOT_FOUND), "{err}");
    }

    #[tokio::test]
    async fn reports_a_missing_hot_head_as_an_http_error() {
        let pool = Pool::SEPOLIA_ETHER_01;
        let manifest = serde_json::json!({
            "availableProtocols": {
                stream_key(&pool): {
                    "protocol": "tornado-cash",
                    "chainId": format!("{:#x}", pool.chain_id),
                    "trackedAddresses": [pool.address],
                    "trackedEventTopics": [],
                    "chunks": [],
                    "hotHead": {
                        "fromBlock": format!("{:#x}", pool.deployed_block),
                        "toBlock": format!("{:#x}", pool.deployed_block + 1),
                        "file": HOT_HEAD_FILE,
                        "size": "0x0",
                        "digest": { "type": "sha256", "data": B256::ZERO },
                    },
                },
            },
        });
        let base_url = serve(vec![
            ("/index.json".into(), StatusCode::OK, manifest.to_string()),
            (
                format!("/{HOT_HEAD_FILE}"),
                StatusCode::NOT_FOUND,
                NO_SUCH_KEY.into(),
            ),
        ]);

        let err = loopback_syncer(&base_url)
            .sync(&pool, 0, u64::MAX)
            .await
            .unwrap_err();

        assert_eq!(http_status(&err), Some(StatusCode::NOT_FOUND), "{err}");
    }

    /// Returns a syncer for `base_url` that ignores proxy settings, so requests reach the
    /// loopback server.
    fn loopback_syncer(base_url: &str) -> SagaSyncer {
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        SagaSyncer::with_client(client, base_url)
    }

    fn http_status(err: &SagaSyncError) -> Option<StatusCode> {
        match err {
            SagaSyncError::Http(err) => err.status(),
            _ => None,
        }
    }

    /// Plays `responses` in order on a loopback port, one per connection, and returns its base
    /// URL.
    ///
    /// Asserts that each request line asks for the path its response is scripted for.
    fn serve(responses: Vec<(String, StatusCode, String)>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());

        thread::spawn(move || {
            for ((path, status, body), stream) in responses.into_iter().zip(listener.incoming()) {
                let mut stream = stream.unwrap();
                let mut lines = BufReader::new(&stream).lines();
                let request_line = lines.next().unwrap().unwrap();
                assert_eq!(request_line, format!("GET {path} HTTP/1.1"));
                for line in lines {
                    if line.unwrap().is_empty() {
                        break;
                    }
                }
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });

        base_url
    }
}
