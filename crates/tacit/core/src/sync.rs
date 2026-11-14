//! Following the pool over JSON-RPC.
//!
//! Confirmed history is kept; the newest blocks are read again each time. Every batch is checked
//! against the pool (`rootSize`) before it is kept, so a node's logs (or a relayer's feed) cannot
//! forge a leaf, and each read ends by checking that the history reaches the pool's head.

use std::time::Duration;

use alloy::{
    primitives::B256, providers::Provider, rpc::types::Filter, sol_types::SolEvent,
    transports::TransportError,
};
use kohaku_rpc_provider::{StreamedEvent, StreamedEventConfig};
use ruint::aliases::U256;
use tracing::{debug, warn};

use crate::{
    abi::TacitPool,
    event::{LogDecodeError, TransactEvent},
    state::{StateError, WalletState},
    tree::IncTree,
    wallet::Wallet,
};

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SyncError {
    #[error("RPC error: {0}")]
    Rpc(#[from] TransportError),
    #[error("contract call failed: {0}")]
    Contract(#[from] alloy::contract::Error),
    #[error(transparent)]
    Decode(#[from] LogDecodeError),
    #[error(transparent)]
    State(#[from] StateError),
    #[error("the history read does not match the pool")]
    Mismatch,
    #[error("the history read ends short of the pool's head; call reset() and sync again")]
    Short,
}

/// The pool's current head: its root and leaf count.
///
/// # Errors
/// Returns an error if the call fails.
pub async fn pool_head<P: Provider>(
    provider: &P,
    pool: alloy::primitives::Address,
) -> Result<(U256, u64), SyncError> {
    let head = TacitPool::new(pool, provider).head().call().await?;
    let size = u64::try_from(head.size).map_err(|_| SyncError::Mismatch)?;
    Ok((U256::from_be_bytes(head.root.0), size))
}

/// How many leaves the tree had when `root` was the pool's head; an error for a root it never
/// held.
///
/// # Errors
/// Returns an error if the call fails or reverts.
pub async fn root_size<P: Provider>(
    provider: &P,
    pool: alloy::primitives::Address,
    root: U256,
) -> Result<U256, SyncError> {
    Ok(TacitPool::new(pool, provider)
        .rootSize(B256::from(root.to_be_bytes::<32>()))
        .call()
        .await?)
}

fn by_position(a: &TransactEvent, b: &TransactEvent) -> std::cmp::Ordering {
    (a.block, a.first_index).cmp(&(b.block, b.first_index))
}

impl Wallet {
    /// Reads what is new in the pool.
    ///
    /// # Errors
    /// Returns an error if a read fails, the history does not match the pool, or it ends short of
    /// the pool's head after three tries (a gap: call [`Wallet::reset`] and sync again).
    pub async fn sync<P: Provider + Clone>(&mut self, provider: &P) -> Result<(), SyncError> {
        for attempt in 0.. {
            let failure = match self.read(provider).await {
                Ok(()) if self.reaches_head(provider).await? => return Ok(()),
                Ok(()) => SyncError::Short,
                Err(SyncError::State(StateError::OutOfOrder { expected, got })) => {
                    SyncError::State(StateError::OutOfOrder { expected, got })
                }
                Err(other) => return Err(other),
            };
            if attempt >= 2 {
                return Err(failure);
            }
            warn!(%failure, attempt, "history incomplete, reading again");
            crate::time::sleep(Duration::from_millis(1500)).await;
        }
        unreachable!()
    }

    /// Applies confirmed history from another source, such as a relayer's feed: `events` in
    /// chain order, through block `through`. The resulting tree is checked against the pool
    /// before it is kept.
    ///
    /// # Errors
    /// Returns an error if the events do not continue the saved state or the pool never held the
    /// resulting tree.
    pub async fn apply_confirmed<P: Provider>(
        &mut self,
        provider: &P,
        events: &[TransactEvent],
        through: u64,
    ) -> Result<(), SyncError> {
        let mut events = events.to_vec();
        events.sort_by(by_position);
        let next = self.saved.absorb(&events, &self.account, self.asset)?;
        self.check(provider, &next.tree, &self.saved.tree).await?;
        self.saved = WalletState {
            block: through.max(self.saved.block),
            ..next
        };
        self.view = self.saved.clone();
        Ok(())
    }

    async fn read<P: Provider + Clone>(&mut self, provider: &P) -> Result<(), SyncError> {
        let tip = provider.get_block_number().await?;
        let safe = tip.saturating_sub(self.pool.confirmations);
        let from = self.saved.block + 1;
        if from > tip {
            self.view = self.saved.clone();
            return Ok(());
        }

        let filter = Filter::new()
            .address(self.pool.address)
            .event_signature(TacitPool::Transact::SIGNATURE_HASH)
            .from_block(from)
            .to_block(tip);
        let logs = StreamedEvent::new(provider.clone(), filter)
            .config(StreamedEventConfig {
                chunk_size: self.pool.log_chunk,
                ..Default::default()
            })
            .query()
            .await?;
        debug!(from, tip, logs = logs.len(), "read pool logs");

        let mut events = Vec::with_capacity(logs.len());
        for log in &logs {
            // Not every node applies the filter's topics; anything else is skipped here.
            if log.address() != self.pool.address {
                continue;
            }
            match TransactEvent::try_from(log) {
                Ok(event) => events.push(event),
                Err(LogDecodeError::NotTransact) => {}
                Err(error) => return Err(error.into()),
            }
        }
        events.sort_by(by_position);
        let split = events.partition_point(|e| e.block <= safe);

        if safe >= from {
            let next = self
                .saved
                .absorb(&events[..split], &self.account, self.asset)?;
            self.check(provider, &next.tree, &self.saved.tree).await?;
            self.saved = WalletState {
                block: safe,
                ..next
            };
        }
        self.view = WalletState {
            block: tip,
            ..self
                .saved
                .absorb(&events[split..], &self.account, self.asset)?
        };
        Ok(())
    }

    /// The leaves a batch added must be ones the pool has held, at exactly that count.
    async fn check<P: Provider>(
        &self,
        provider: &P,
        next: &IncTree,
        before: &IncTree,
    ) -> Result<(), SyncError> {
        if next.size() == before.size() {
            return Ok(());
        }
        let size = root_size(provider, self.pool.address, next.root())
            .await
            .map_err(|_| SyncError::Mismatch)?;
        if size == U256::from(next.size()) {
            Ok(())
        } else {
            Err(SyncError::Mismatch)
        }
    }

    /// Whether the view reaches the pool's head. A node that answers from a block behind the
    /// logs is not a gap.
    async fn reaches_head<P: Provider>(&self, provider: &P) -> Result<bool, SyncError> {
        let (root, size) = pool_head(provider, self.pool.address).await?;
        let tree = &self.view.tree;
        Ok(size < tree.size() || (size == tree.size() && root == tree.root()))
    }
}
