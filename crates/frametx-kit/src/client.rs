use std::time::Duration;

use alloy::primitives::{Address, B256, Bytes, U256};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::tx::FrameTx;

const RPC_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug)]
pub struct FrameTxClient {
    rpc: Url,
    http: reqwest::Client,
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("rpc error: {0}")]
    Rpc(String),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error("simulate unavailable on this endpoint")]
    SimulateUnavailable,
    #[error("invalid simulation: {0}")]
    InvalidSim(String),
    #[error("sender frame reverts: {0}")]
    SenderRevert(String),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulateResult {
    pub valid: Option<bool>,
    pub prefix_shape: Option<String>,
    pub payer: Option<String>,
    pub execution_status: Option<String>,
    pub execution_error: Option<String>,
    pub violation: Option<Value>,
    pub gas_used: Option<String>,
    pub frames: Option<Vec<SimulateFrame>>,
    #[serde(flatten, default)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulateFrame {
    pub gas_used: Option<String>,
    pub succeeded: Option<bool>,
    #[serde(flatten, default)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Serialize)]
struct RpcReq<'a> {
    jsonrpc: &'a str,
    id: u64,
    method: &'a str,
    params: Value,
}

impl FrameTxClient {
    #[must_use]
    pub fn new(rpc: Url) -> Self {
        Self {
            rpc,
            http: reqwest::Client::builder()
                .timeout(RPC_TIMEOUT)
                .build()
                .expect("reqwest client"),
        }
    }

    /// # Errors
    /// Returns if the RPC call fails or the response is malformed.
    pub async fn rpc(&self, method: &str, params: Value) -> Result<Value, ClientError> {
        let body = RpcReq {
            jsonrpc: "2.0",
            id: 1,
            method,
            params,
        };
        let resp: Value = self
            .http
            .post(self.rpc.clone())
            .json(&body)
            .send()
            .await?
            .json()
            .await?;
        if let Some(err) = resp.get("error") {
            return Err(ClientError::Rpc(err.to_string()));
        }
        Ok(resp.get("result").cloned().unwrap_or(Value::Null))
    }

    /// # Errors
    /// Returns if the RPC call fails.
    pub async fn chain_id(&self) -> Result<u64, ClientError> {
        let v = self.rpc("eth_chainId", json!([])).await?;
        parse_hex_u64(&v).ok_or_else(|| ClientError::Rpc("bad chain id".into()))
    }

    /// # Errors
    /// Returns if the RPC call fails or the slot is not 32 bytes.
    pub async fn storage_at(&self, addr: Address, slot: B256) -> Result<B256, ClientError> {
        let v = self
            .rpc(
                "eth_getStorageAt",
                json!([format!("{addr:#x}"), format!("{slot:#x}"), "latest"]),
            )
            .await?;
        let s = v
            .as_str()
            .ok_or_else(|| ClientError::Rpc("bad storage value".into()))?;
        let bytes =
            hex::decode(s.trim_start_matches("0x")).map_err(|e| ClientError::Rpc(e.to_string()))?;
        if bytes.len() > 32 {
            return Err(ClientError::Rpc(
                "storage value longer than 32 bytes".into(),
            ));
        }
        let mut out = [0u8; 32];
        out[32 - bytes.len()..].copy_from_slice(&bytes);
        Ok(B256::from(out))
    }

    /// # Errors
    /// Returns if the RPC call fails.
    pub async fn tx_count(&self, addr: Address) -> Result<u64, ClientError> {
        let v = self
            .rpc(
                "eth_getTransactionCount",
                json!([format!("{addr:#x}"), "latest"]),
            )
            .await?;
        parse_hex_u64(&v).ok_or_else(|| ClientError::Rpc("bad nonce".into()))
    }

    /// # Errors
    /// Returns if the RPC call fails.
    pub async fn fees(&self) -> Result<(U256, U256), ClientError> {
        let blk = self
            .rpc("eth_getBlockByNumber", json!(["latest", false]))
            .await?;
        let base = blk
            .get("baseFeePerGas")
            .and_then(parse_hex_u256)
            .unwrap_or(U256::ZERO);
        let tip = U256::from(1_000_000_000u64);
        Ok((tip, base * U256::from(2) + tip))
    }

    /// # Errors
    /// Returns if the RPC call fails.
    pub async fn slot_number(&self) -> Result<u64, ClientError> {
        let blk = self
            .rpc("eth_getBlockByNumber", json!(["latest", false]))
            .await?;
        blk.get("slotNumber")
            .and_then(parse_hex_u64)
            .ok_or_else(|| ClientError::Rpc("latest block has no EIP-7843 slotNumber".into()))
    }

    /// EIP-7843 `slotNumber` of the block with `block_hash`.
    ///
    /// # Errors
    /// Returns if the RPC call fails or the block has no slot.
    pub async fn slot_number_of(&self, block_hash: B256) -> Result<u64, ClientError> {
        let blk = self
            .rpc(
                "eth_getBlockByHash",
                json!([format!("{block_hash:#x}"), false]),
            )
            .await?;
        blk.get("slotNumber")
            .and_then(parse_hex_u64)
            .ok_or_else(|| {
                ClientError::Rpc(format!("block {block_hash:#x} has no EIP-7843 slotNumber"))
            })
    }

    /// Poll `eth_getTransactionReceipt` until it is present.
    ///
    /// # Errors
    /// Returns if the RPC fails or the wait exceeds `attempts`.
    pub async fn wait_receipt(&self, hash: B256, attempts: u32) -> Result<Value, ClientError> {
        for _ in 0..attempts {
            let v = self
                .rpc("eth_getTransactionReceipt", json!([format!("{hash:#x}")]))
                .await?;
            if !v.is_null() {
                return Ok(v);
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        Err(ClientError::Rpc(format!("timed out waiting for {hash:#x}")))
    }

    /// Latest block number.
    ///
    /// # Errors
    /// Returns if the RPC call fails.
    pub async fn block_number(&self) -> Result<u64, ClientError> {
        let v = self.rpc("eth_blockNumber", json!([])).await?;
        parse_hex_u64(&v).ok_or_else(|| ClientError::Rpc("bad block number".into()))
    }

    /// # Errors
    /// Returns if the RPC fails for a reason other than a missing method.
    pub async fn simulate(&self, raw: &Bytes) -> Result<Option<SimulateResult>, ClientError> {
        let body = RpcReq {
            jsonrpc: "2.0",
            id: 1,
            method: "ethrex_simulateFrameTransaction",
            params: json!([format!("0x{}", hex::encode(raw))]),
        };
        let resp: Value = self
            .http
            .post(self.rpc.clone())
            .json(&body)
            .send()
            .await?
            .json()
            .await?;
        if let Some(err) = resp.get("error") {
            if err.get("code").and_then(Value::as_i64) == Some(-32601) {
                return Ok(None);
            }
            return Err(ClientError::Rpc(err.to_string()));
        }
        let result = resp.get("result").cloned().unwrap_or(Value::Null);
        Ok(Some(
            serde_json::from_value(result).map_err(|e| ClientError::Rpc(e.to_string()))?,
        ))
    }

    /// # Errors
    /// Returns if send fails.
    pub async fn send_raw(&self, raw: &Bytes) -> Result<B256, ClientError> {
        let v = self
            .rpc(
                "eth_sendRawTransaction",
                json!([format!("0x{}", hex::encode(raw))]),
            )
            .await?;
        let s = v
            .as_str()
            .ok_or_else(|| ClientError::Rpc("bad tx hash".into()))?;
        let bytes =
            hex::decode(s.trim_start_matches("0x")).map_err(|e| ClientError::Rpc(e.to_string()))?;
        if bytes.len() != 32 {
            return Err(ClientError::Rpc("tx hash not 32 bytes".into()));
        }
        Ok(B256::from_slice(&bytes))
    }

    /// Simulate a spend and refuse to send if the prefix is invalid or a SENDER frame reverts.
    ///
    /// # Errors
    /// Returns if simulation is missing for a spend, invalid, or a later frame reverts.
    pub async fn gate_spend(&self, tx: &FrameTx) -> Result<SimulateResult, ClientError> {
        let raw = tx.raw();
        let sim = self.simulate(&raw).await?;
        let Some(sim) = sim else {
            return Err(ClientError::SimulateUnavailable);
        };
        if sim.valid != Some(true) {
            let dump = serde_json::to_string(&sim).unwrap_or_else(|_| {
                sim.violation
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default()
            });
            let violation = sim
                .violation
                .as_ref()
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| sim.violation.as_ref().map(ToString::to_string))
                .unwrap_or_else(|| "invalid".into());
            return Err(ClientError::InvalidSim(format!(
                "{violation}; execution_status={:?} execution_error={:?} prefix_shape={:?} frames={:?} dump={dump}",
                sim.execution_status, sim.execution_error, sim.prefix_shape, sim.frames
            )));
        }
        if let Some(status) = &sim.execution_status
            && status != "success"
        {
            let dump = serde_json::to_string(&sim).unwrap_or_default();
            return Err(ClientError::SenderRevert(format!(
                "{}; execution_error={:?} frames={:?} dump={dump}",
                status, sim.execution_error, sim.frames
            )));
        }
        Ok(sim)
    }
}

fn parse_hex_u64(v: &Value) -> Option<u64> {
    let s = v.as_str()?;
    u64::from_str_radix(s.trim_start_matches("0x"), 16).ok()
}

fn parse_hex_u256(v: &Value) -> Option<U256> {
    let s = v.as_str()?;
    U256::from_str_radix(s.trim_start_matches("0x"), 16).ok()
}
