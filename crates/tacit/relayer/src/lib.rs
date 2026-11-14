#![doc = include_str!("../README.md")]

use std::{str::FromStr, time::Duration};

use alloy::{
    primitives::{Address, B256, Bytes},
    providers::Provider,
};
use kohaku_tacit::{
    IncTree, Payer, Pool, RelayTx, RelayerConfig, SyncError, TransactEvent, TreeError, Wallet,
};
use reqwest::{Response, StatusCode};
use ruint::aliases::U256;
use serde::{Deserialize, de::DeserializeOwned};
use tracing::debug;

const NO_ANSWER: &str = "the relayer did not answer, and it may still have sent the spend; check \
                         the pool before sending again";
const GATEWAY_FAILURES: [u16; 8] = [502, 503, 504, 520, 521, 522, 523, 524];
const READ_TIMEOUT: Duration = Duration::from_secs(30);
const RELAY_TIMEOUT: Duration = Duration::from_mins(3);

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RelayerError {
    #[error("this pool has no relayer configured")]
    NotConfigured,
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("the relayer quoted another chain or pool")]
    WrongPool,
    #[error("the relayer quoted address {0}, not the configured one")]
    WrongAddress(Address),
    #[error("the relayer fee {quoted} is above the limit {limit}")]
    FeeAboveLimit { quoted: u128, limit: u128 },
    #[error("the relayer needs a fee of {needed} (offered {offered}); prepare the spend again")]
    FeeTooLow { needed: u128, offered: u128 },
    #[error("{NO_ANSWER}")]
    NoAnswer,
    #[error("those funds were already spent; sync and prepare the operation again")]
    AlreadySpent,
    #[error("a spend of these funds is already on its way")]
    InFlight,
    #[error("the proof is stale ({0}); sync and prepare the operation again")]
    Stale(String),
    #[error("the relayer refused ({status}): {message}")]
    Refused { status: u16, message: String },
    #[error("the relayer sent a malformed answer: {0}")]
    Malformed(&'static str),
    #[error(transparent)]
    Sync(#[from] SyncError),
    #[error(transparent)]
    Tree(#[from] TreeError),
}

/// A relayer's price for one spend, and the address the proof must name as `relayer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quote {
    pub relayer: Address,
    pub fee: u128,
}

impl Quote {
    /// The payer to build the spend with.
    #[must_use]
    pub fn payer(&self) -> Payer {
        Payer {
            relayer: self.relayer,
            fee: self.fee,
        }
    }
}

/// The relayer's view of the pool: its head, the insertions queued after it, and the tail they
/// lead to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    pub root: U256,
    pub size: u64,
    /// Each queued spend's two output leaves, in order.
    pub pending: Vec<[U256; 2]>,
    pub tail_root: U256,
    pub tail_size: u64,
}

/// One page of the relayer's confirmed history.
#[derive(Debug, Clone)]
pub struct FeedPage {
    pub events: Vec<TransactEvent>,
    /// The last block the page covers.
    pub through: u64,
}

/// A decimal or `0x`-hex integer, as the relayer writes them.
fn parse_u256(s: &str) -> Result<U256, RelayerError> {
    let parsed = match s.strip_prefix("0x") {
        Some(hex) => U256::from_str_radix(hex, 16),
        None => U256::from_str(s),
    };
    parsed.map_err(|_| RelayerError::Malformed("not an integer"))
}

fn parse_u64(s: &str) -> Result<u64, RelayerError> {
    u64::try_from(parse_u256(s)?).map_err(|_| RelayerError::Malformed("integer too large"))
}

fn parse_u128(s: &str) -> Result<u128, RelayerError> {
    u128::try_from(parse_u256(s)?).map_err(|_| RelayerError::Malformed("integer too large"))
}

fn need(x: Option<&Num>) -> Result<&Num, RelayerError> {
    x.ok_or(RelayerError::Malformed("event field"))
}

/// A JSON number or string.
#[derive(Deserialize)]
#[serde(untagged)]
enum Num {
    Int(u64),
    Text(String),
}

impl Num {
    fn u64(&self) -> Result<u64, RelayerError> {
        match self {
            Self::Int(x) => Ok(*x),
            Self::Text(s) => parse_u64(s),
        }
    }

    fn u256(&self) -> Result<U256, RelayerError> {
        match self {
            Self::Int(x) => Ok(U256::from(*x)),
            Self::Text(s) => parse_u256(s),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuoteWire {
    chain_id: Num,
    pool: Address,
    relayer: String,
    fee: Num,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PendingWire {
    out_leaf0: Num,
    out_leaf1: Num,
}

#[derive(Deserialize)]
struct TailWire {
    root: Num,
    size: Num,
}

#[derive(Deserialize)]
struct HeadWire {
    root: Num,
    size: Num,
    #[serde(default)]
    pending: Vec<PendingWire>,
    tail: TailWire,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EventWire {
    kind: String,
    block: u64,
    #[serde(default)]
    first_index: Option<Num>,
    #[serde(default)]
    nf0: Option<Num>,
    #[serde(default)]
    nf1: Option<Num>,
    #[serde(default)]
    out_leaf0: Option<Num>,
    #[serde(default)]
    out_leaf1: Option<Num>,
    #[serde(default)]
    memo0: Option<Bytes>,
    #[serde(default)]
    memo1: Option<Bytes>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FeedWire {
    chain_id: Num,
    pool: Address,
    through: Num,
    events: Vec<EventWire>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct RelayAnswer {
    tx_hash: Option<B256>,
    error: Option<String>,
    #[serde(default)]
    stale: bool,
    need_fee: Option<String>,
}

/// The HTTP client of one pool's relayer.
#[derive(Debug, Clone)]
pub struct RelayerClient {
    http: reqwest::Client,
    chain_id: u64,
    pool: Address,
    config: RelayerConfig,
}

impl RelayerClient {
    /// The client of `pool`'s configured relayer.
    ///
    /// # Errors
    /// Returns an error if the pool has no relayer.
    pub fn new(pool: &Pool) -> Result<Self, RelayerError> {
        let config = pool.relayer.clone().ok_or(RelayerError::NotConfigured)?;
        Ok(Self {
            http: reqwest::Client::new(),
            chain_id: pool.chain_id,
            pool: pool.address,
            config,
        })
    }

    #[must_use]
    pub fn url(&self) -> &str {
        &self.config.url
    }

    // A busy relayer answers 429 with Retry-After: wait what it says (10 s if unsaid, at most
    // 30) and ask again.
    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, RelayerError> {
        let url = format!("{}{path}", self.config.url);
        for attempt in 0.. {
            let response = self.http.get(&url).timeout(READ_TIMEOUT).send().await?;
            if response.status() != StatusCode::TOO_MANY_REQUESTS || attempt >= 4 {
                return answer(response).await;
            }
            let after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .filter(|s| *s > 0)
                .unwrap_or(10)
                .min(30);
            let jitter = 0.5 + rand::random::<f64>();
            debug!(after, attempt, "relayer busy");
            kohaku_tacit::sleep(Duration::from_secs(after).mul_f64(jitter)).await;
        }
        unreachable!()
    }

    /// The relayer's quote, checked before anything is signed: this chain and pool, the expected
    /// address, a fee within the ceiling.
    ///
    /// # Errors
    /// Returns an error if the request fails or the quote does not pass the checks.
    pub async fn quote(&self) -> Result<Quote, RelayerError> {
        let q: QuoteWire = self.get("/quote").await?;
        if q.chain_id.u64()? != self.chain_id || q.pool != self.pool {
            return Err(RelayerError::WrongPool);
        }
        let relayer = Address::from_str(&q.relayer)
            .map_err(|_| RelayerError::Malformed("relayer address"))?;
        if relayer != self.config.address {
            return Err(RelayerError::WrongAddress(relayer));
        }
        let fee = u128::try_from(q.fee.u256()?).map_err(|_| RelayerError::Malformed("fee"))?;
        if fee > self.config.max_fee {
            return Err(RelayerError::FeeAboveLimit {
                quoted: fee,
                limit: self.config.max_fee,
            });
        }
        Ok(Quote { relayer, fee })
    }

    /// The pool's head as the relayer sees it, with the insertions it has queued.
    ///
    /// # Errors
    /// Returns an error if the request fails or the answer is malformed.
    pub async fn head(&self) -> Result<Head, RelayerError> {
        let h: HeadWire = self.get("/head").await?;
        Ok(Head {
            root: h.root.u256()?,
            size: h.size.u64()?,
            pending: h
                .pending
                .iter()
                .map(|p| Ok([p.out_leaf0.u256()?, p.out_leaf1.u256()?]))
                .collect::<Result<_, RelayerError>>()?,
            tail_root: h.tail.root.u256()?,
            tail_size: h.tail.size.u64()?,
        })
    }

    /// The relayer's confirmed history from block `from`.
    ///
    /// # Errors
    /// Returns an error if the request fails or the page is for another pool.
    pub async fn events(&self, from: u64) -> Result<FeedPage, RelayerError> {
        let feed: FeedWire = self.get(&format!("/events?from={from}")).await?;
        if feed.chain_id.u64()? != self.chain_id || feed.pool != self.pool {
            return Err(RelayerError::WrongPool);
        }
        let mut events = Vec::new();
        for e in feed.events.iter().filter(|e| e.kind == "transact") {
            events.push(TransactEvent {
                nullifiers: [need(e.nf0.as_ref())?.u256()?, need(e.nf1.as_ref())?.u256()?],
                out_leaves: [
                    need(e.out_leaf0.as_ref())?.u256()?,
                    need(e.out_leaf1.as_ref())?.u256()?,
                ],
                first_index: need(e.first_index.as_ref())?.u64()?,
                memos: [
                    e.memo0.clone().unwrap_or_default(),
                    e.memo1.clone().unwrap_or_default(),
                ],
                block: e.block,
            });
        }
        Ok(FeedPage {
            events,
            through: feed.through.u64()?,
        })
    }

    /// Reads confirmed history from the relayer's feed into `wallet`, a page at a time, through
    /// the provider's safe block. Each page is checked against the pool before it is kept. Call
    /// [`Wallet::sync`] after it for the newest blocks.
    ///
    /// # Errors
    /// Returns an error if a request fails or a page does not match the pool.
    pub async fn sync_from_feed<P: Provider>(
        &self,
        wallet: &mut Wallet,
        provider: &P,
    ) -> Result<(), RelayerError> {
        let tip = provider
            .get_block_number()
            .await
            .map_err(|e| RelayerError::Sync(SyncError::Rpc(e)))?;
        let safe = tip.saturating_sub(wallet.pool().confirmations);
        loop {
            let from = wallet.save().block + 1;
            if from > safe {
                return Ok(());
            }
            let page = self.events(from).await?;
            let through = page.through.min(safe);
            if through < from {
                return Ok(());
            }
            let events: Vec<_> = page
                .events
                .into_iter()
                .filter(|e| e.block >= from && e.block <= through)
                .collect();
            wallet.apply_confirmed(provider, &events, through).await?;
        }
    }

    /// The tree a relayed spend is proven against: the wallet's, plus the insertions the relayer
    /// has queued after the pool's head when they lead to the tail it announces; otherwise the
    /// wallet's own.
    ///
    /// Use it for spends that insert notes. A spend that inserts nothing (a withdrawal without
    /// change) proves membership against a root the pool has held, so it never follows the queue.
    pub async fn proving_tree<P: Provider + Clone>(
        &self,
        wallet: &mut Wallet,
        provider: &P,
    ) -> IncTree {
        let Ok(head) = self.head().await else {
            return wallet.tree().clone();
        };
        // The pool may have moved since the last read: read once more before comparing.
        if head.root != wallet.tree().root() && wallet.sync(provider).await.is_err() {
            return wallet.tree().clone();
        }
        if head.root != wallet.tree().root() {
            return wallet.tree().clone();
        }
        let mut queued = wallet.tree().clone();
        let leaves: Vec<U256> = head.pending.iter().flatten().copied().collect();
        if queued.append_leaves(&leaves).is_err() || queued.root() != head.tail_root {
            return wallet.tree().clone();
        }
        queued
    }

    /// Submits a proven spend. Returns the transaction hash.
    ///
    /// # Errors
    /// Returns an error if the relayer refuses the spend or does not answer. A missing answer
    /// ([`RelayerError::NoAnswer`]) does not mean the spend was not sent.
    pub async fn relay(&self, tx: &RelayTx) -> Result<B256, RelayerError> {
        let url = format!("{}/relay", self.config.url);
        let body = serde_json::json!({ "tx": tx });
        let Ok(response) = self
            .http
            .post(&url)
            .json(&body)
            .timeout(RELAY_TIMEOUT)
            .send()
            .await
        else {
            // A request that got no answer may still have been sent.
            return Err(RelayerError::NoAnswer);
        };
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        let answer: RelayAnswer = serde_json::from_str(&text).unwrap_or_default();
        relay_outcome(status, &answer, tx)
    }
}

fn relay_outcome(
    status: StatusCode,
    answer: &RelayAnswer,
    tx: &RelayTx,
) -> Result<B256, RelayerError> {
    if status.is_success()
        && let Some(hash) = answer.tx_hash
    {
        return Ok(hash);
    }
    let why = answer.error.clone().unwrap_or_default();
    // A gateway failure with no answer from the relayer says nothing about whether it sent.
    if GATEWAY_FAILURES.contains(&status.as_u16()) && why.is_empty() {
        return Err(RelayerError::NoAnswer);
    }
    if why.contains("AlreadyNullified") || why.contains("already spent") {
        return Err(RelayerError::AlreadySpent);
    }
    if why.contains("already being relayed") {
        return Err(RelayerError::InFlight);
    }
    if status == StatusCode::CONFLICT && answer.stale {
        return Err(RelayerError::Stale(why));
    }
    if let Some(needed) = answer.need_fee.as_deref().and_then(|n| parse_u128(n).ok()) {
        return Err(RelayerError::FeeTooLow {
            needed,
            offered: parse_u128(&tx.fee).unwrap_or_default(),
        });
    }
    Err(RelayerError::Refused {
        status: status.as_u16(),
        message: if why.is_empty() {
            format!("relayer returned {status}")
        } else {
            why
        },
    })
}

async fn answer<T: DeserializeOwned>(response: Response) -> Result<T, RelayerError> {
    let status = response.status();
    let text = response.text().await?;
    if !status.is_success() {
        let message = serde_json::from_str::<RelayAnswer>(&text)
            .ok()
            .and_then(|a| a.error)
            .unwrap_or_else(|| format!("relayer returned {status}"));
        return Err(RelayerError::Refused {
            status: status.as_u16(),
            message,
        });
    }
    serde_json::from_str(&text).map_err(|_| RelayerError::Malformed("unexpected JSON"))
}

#[cfg(test)]
mod tests;
