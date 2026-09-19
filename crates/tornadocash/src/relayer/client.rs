use alloy::primitives::{Address, B256, Bytes, TxHash};
use serde::{Deserialize, Serialize};

use crate::{abis::tornado::Tornado, pool::Pool, relayer::status::RelayerStatus};

/// Tornadocash relayer client.
///
/// Written against the [tornado-relayer v5](https://github.com/tornado-dao/tornado-relayer/tree/mainnet-v5)
/// reference implementation.
#[derive(Clone)]
pub struct RelayerClient {
    pub url: String,
    pub net_id: u32,

    client: reqwest::Client,
}

#[derive(Debug, thiserror::Error)]
pub enum RelayerClientError {
    #[error("Relayer request failed: {0}")]
    RequestFailed(#[from] reqwest::Error),
    #[error("Relayer returned an error: {0}")]
    RelayerError(String),
    #[error("Provider error: {0}")]
    Provider(#[from] alloy::transports::RpcError<alloy::transports::TransportErrorKind>),
}

#[derive(Debug, Clone)]
pub struct JobReceipt {
    pub id: JobId,
    pub nullifier_hash: B256,
    pub pool: Pool,
}

/// Withdraw job ID
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobId(pub(super) String);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WithdrawRequest {
    pub contract: Address,
    pub proof: Bytes,
    /// (`root`, `nullifier_hash`, `recipient`, `relayer`, `fee`, `refund`).
    ///
    /// Uses B256 for `fee` and `refund` so they are serialized as 32-byte hex strings.
    pub args: (B256, B256, Address, Address, B256, B256),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WithdrawResponse {
    pub id: JobId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub fn new(url: &str, net_id: u32) -> Self {
        Self {
            url: url.to_string(),
            net_id,
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
        pool: &Pool,
        call: Tornado::withdrawCall,
    ) -> Result<JobReceipt, RelayerClientError> {
        let nullifier_hash = call._nullifierHash;
        let request = WithdrawRequest {
            contract: pool.address,
            proof: call._proof,
            args: (
                call._root,
                call._nullifierHash,
                call._recipient,
                call._relayer,
                call._fee.into(),
                call._refund.into(),
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
            pool: *pool,
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
