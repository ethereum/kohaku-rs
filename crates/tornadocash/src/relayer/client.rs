use std::collections::HashMap;

use alloy::primitives::{Address, B256, Bytes, TxHash};
use ruint::aliases::U256;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::{
    abis::tornado::Tornado,
    pool::{Asset, Pool},
};

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

/// Relayer status response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayerStatus {
    pub reward_account: Address,
    pub instances: HashMap<String, Instance>,
    pub net_id: u32,
    pub eth_prices: HashMap<String, U256>,
    pub tornado_service_fee: f64,
    pub mining_service_fee: f64,
    pub version: String,
    pub health: Health,
    pub current_queue: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub instance_address: HashMap<String, Address>,
    pub symbol: String,
    pub decimals: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Health {
    pub status: Option<String>,
    pub error: Option<String>,
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
struct ErrorResponse {
    pub error: String,
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
        info!("Fetching relayer status from {}", url);

        let response = self
            .client
            .get(&url)
            .send()
            .await?
            .json::<RelayerStatus>()
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
        let response = self.client.post(&url).json(&request).send().await?;

        if !response.status().is_success() {
            let error = response
                .json::<ErrorResponse>()
                .await
                .map_or_else(|_| "unknown error".to_string(), |body| body.error);
            return Err(RelayerClientError::RelayerError(error));
        }

        let id = response.json::<WithdrawResponse>().await?.id;
        Ok(JobReceipt {
            id,
            nullifier_hash,
            pool: *pool,
        })
    }

    /// Polls the relayer for a withdrawal job's status.
    ///
    /// See <https://github.com/tornado-dao/tornado-relayer/blob/52473197ea49fb70dab8fead01de52545801ca6b/src/contollers/status.js#L32>
    /// for the reference implementation.
    pub async fn job_status(
        &self,
        receipt: &JobReceipt,
    ) -> Result<JobResponse, RelayerClientError> {
        let url = format!("{}/v1/jobs/{}", self.url, receipt.id.0);
        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            let error = response
                .json::<ErrorResponse>()
                .await
                .map_or_else(|_| "unknown error".to_string(), |body| body.error);
            return Err(RelayerClientError::RelayerError(error));
        }

        let job = response.json::<JobResponse>().await?;
        Ok(job)
    }
}

impl RelayerStatus {
    /// Checks if the relayer supports the given pool.
    pub fn supports(&self, pool: &Pool) -> bool {
        let Some(instance) = self.instances.get(&pool.symbol()) else {
            return false;
        };

        for (amount, address) in &instance.instance_address {
            if amount != &pool.amount() {
                continue;
            }
            if address != &pool.address {
                continue;
            }
            return true;
        }

        return false;
    }

    /// Calculates the fee for a transaction.
    ///
    /// Returns `None` if the relayer does not support the given pool.
    pub fn fee(&self, pool: &Pool, gas_price: u128, amount: U256, refund: U256) -> Option<U256> {
        if !self.supports(pool) {
            return None;
        }

        // Scale the fee percentage into a fixed-point integer.
        const FEE_PRECISION: u64 = 1_000_000;

        let fee_scaled = (self.tornado_service_fee / 100.0 * FEE_PRECISION as f64).round() as u64;
        let fee_percent = (amount * U256::from(fee_scaled)) / U256::from(FEE_PRECISION);
        let expense = U256::from(gas_price) * U256::from(500_000);

        // If the asset is native, the fee is `expense + fee_percent`
        if matches!(pool.asset, Asset::Native { .. }) {
            return Some(fee_percent + expense);
        }

        let Some(price) = self.eth_prices.get(&pool.symbol()) else {
            return None;
        };

        // If the asset is non-native, the fee is:
        // `((expense + refund) * 10^decimals / price) + fee_percent`
        Some(
            (expense + refund) * U256::from(10).pow(U256::from(pool.asset.decimals())) / *price
                + fee_percent,
        )
    }
}
