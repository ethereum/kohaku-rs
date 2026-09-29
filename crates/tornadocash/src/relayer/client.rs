use alloy::primitives::{Address, B256, Bytes, TxHash, U256};
use serde::{Deserialize, Serialize};

use crate::{pool::Pool, relayer::status::RelayerStatus, withdrawal::ProvenWithdrawal};

/// Tornadocash relayer client.
///
/// Written against the [tornado-relayer v5](https://github.com/tornado-dao/tornado-relayer/tree/mainnet-v5)
/// reference implementation.
#[derive(Clone)]
pub struct RelayerClient {
    pub url: String,

    client: reqwest::Client,
}

#[derive(Debug, thiserror::Error)]
pub enum RelayerClientError {
    #[error("Relayer request failed: {0}")]
    RequestFailed(#[from] reqwest::Error),
    #[error("Relayer returned an error: {0}")]
    RelayerError(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobReceipt {
    pub id: JobId,
    pub pool: Pool,
    pub nullifier_hash: U256,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobId(pub(super) String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct WithdrawRequest {
    pub contract: Address,
    pub proof: Bytes,
    /// (`root`, `nullifier_hash`, `recipient`, `relayer`, `fee`, `refund`).
    ///
    /// Uses B256 for `fee` and `refund` so they are serialized as 32-byte hex strings.
    pub args: (B256, B256, Address, Address, B256, B256),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct WithdrawResponse {
    pub id: JobId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobResponse {
    pub status: JobStatus,
    pub tx_hash: Option<TxHash>,
    pub confirmations: Option<u32>,
    pub failed_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum JobStatus {
    Queued,
    Accepted,
    Sent,
    Mined,
    Resubmitted,
    Confirmed,
    Failed,
}

impl RelayerClient {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
            client: reqwest::Client::new(),
        }
    }

    /// Retrieves the relayer's status.
    ///
    /// See <https://github.com/tornado-dao/tornado-relayer/blob/52473197ea49fb70dab8fead01de52545801ca6b/src/contollers/status.js#L7>
    /// for the reference implementation.
    pub async fn status(&self) -> Result<RelayerStatus, RelayerClientError> {
        let url = format!("{}/v1/status", self.url);
        let response: RelayerStatus = self
            .client
            .get(&url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(response)
    }

    /// Submits a withdrawal job to the relayer.
    ///
    /// See <https://github.com/tornado-dao/tornado-relayer/blob/52473197ea49fb70dab8fead01de52545801ca6b/src/contollers/controller.js#L9>
    /// for the reference implementation.
    pub async fn withdraw(
        &self,
        withdrawal: ProvenWithdrawal,
    ) -> Result<JobReceipt, RelayerClientError> {
        let nullifier_hash = withdrawal.note.nullifier_hash();
        let request = WithdrawRequest {
            contract: withdrawal.pool.address,
            proof: withdrawal.proof_bytes(),
            args: (
                withdrawal.root.into(),
                nullifier_hash.into(),
                withdrawal.recipient,
                withdrawal.relayer(),
                withdrawal.fee().into(),
                withdrawal.refund().into(),
            ),
        };

        let url = format!("{}/v1/tornadoWithdraw", self.url);
        let response: WithdrawResponse = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(JobReceipt {
            id: response.id,
            nullifier_hash,
            pool: withdrawal.inner.pool,
        })
    }

    /// Polls the relayer for a withdrawal job's status.
    ///
    /// See <https://github.com/tornado-dao/tornado-relayer/blob/52473197ea49fb70dab8fead01de52545801ca6b/src/contollers/status.js#L32>
    /// for the reference implementation.
    pub async fn job_status(&self, id: &JobId) -> Result<JobResponse, RelayerClientError> {
        let url = format!("{}/v1/jobs/{}", self.url, id.0);
        let response: JobResponse = self
            .client
            .get(&url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(response)
    }
}
