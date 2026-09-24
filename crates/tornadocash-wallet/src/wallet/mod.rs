use std::sync::Arc;

use kohaku_kv_store::{Store, backend::StoreError};
use kohaku_tornadocash::{
    note::{Note, Nullifier, Secret},
    pool::Pool,
    provider::{TornadoProvider, TornadoProviderError},
};
use thiserror::Error;
use tokio::sync::Mutex;

use crate::{backend::KeychainError, keychain::Keychain, wallet::store::WalletStoreExt};

mod store;

/// Number of consecutive undeposited nonces after which recovery stops scanning.
const DEFAULT_GAP_LIMIT: u64 = 20;

/// A wallet for tornadocash.
///
/// Manages a keychain and tracks the on-chain status of its derived notes.
///
/// # Example
/// ```rust,no_run
/// # async fn example(wallet: Wallet, pool: Pool) -> Result<(), WalletError> {
/// // List all notes for the pool.
/// let notes = wallet.notes(&pool).await?;
///
/// // Reserve a new note for the pool.
/// let (nonce, secret, nullifier) = wallet.reserve(&pool).await?;
/// # }
/// ```
#[derive(Clone)]
pub struct Wallet {
    provider: TornadoProvider,
    keychain: Keychain,
    store: Store,
    gap_limit: u64,
    nonce_lock: Arc<Mutex<()>>,
}

/// A derived note and its on-chain status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalletNote {
    pub nonce: u64,
    pub note: Note,
    pub status: NoteStatus,
}

/// The status of a derived note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteStatus {
    /// Handed out by a wallet, but not yet seen on-chain.
    Pending,
    Deposited {
        leaf_index: u32,
    },
    Withdrawn {
        leaf_index: u32,
    },
}

#[derive(Debug, Error)]
pub enum WalletError {
    #[error("Keychain error: {0}")]
    Keychain(#[from] KeychainError),
    #[error("Provider error: {0}")]
    Provider(#[from] TornadoProviderError),
    #[error("Store error: {0}")]
    Store(#[from] StoreError),
}

impl Wallet {
    #[must_use]
    pub fn new(provider: TornadoProvider, keychain: Keychain, store: Store) -> Self {
        Self {
            provider,
            keychain,
            store,
            gap_limit: DEFAULT_GAP_LIMIT,
            nonce_lock: Arc::new(Mutex::new(())),
        }
    }

    /// Sets how many consecutive undeposited nonces recovery tolerates before stopping.
    #[must_use]
    pub fn with_gap_limit(mut self, gap_limit: u64) -> Self {
        self.gap_limit = gap_limit;
        self
    }

    /// Syncs and returns the note at `nonce` for `pool`.
    ///
    /// Automatically recover's the wallet's state for `pool` before resolving the note.
    ///
    /// # Errors
    /// Returns an error if the keychain, provider, or store fails.
    pub async fn note(&self, pool: &Pool, nonce: u64) -> Result<WalletNote, WalletError> {
        let _lock = self.nonce_lock.lock().await;
        self.recover(pool).await?;

        self.note_at(pool, nonce).await
    }

    /// Syncs and returns all of this wallet's notes for `pool`.
    ///
    /// Automatically recover's the wallet's state for `pool` before resolving the note.
    ///
    /// # Errors
    /// Returns an error if the keychain, provider, or store fails.
    pub async fn notes(&self, pool: &Pool) -> Result<Vec<WalletNote>, WalletError> {
        let _lock = self.nonce_lock.lock().await;
        let end = self.recover(pool).await?;

        let mut notes = Vec::with_capacity(end as usize);
        for nonce in 0..end {
            notes.push(self.note_at(pool, nonce).await?);
        }

        Ok(notes)
    }

    /// Reserves the next nonce for `pool` and returns its derived material.
    ///
    /// # Errors
    /// Returns an error if the keychain, provider, or store fails.
    pub async fn reserve(&self, pool: &Pool) -> Result<(u64, Secret, Nullifier), WalletError> {
        let _lock = self.nonce_lock.lock().await;

        let nonce = self.recover(pool).await?;
        self.pool_store(pool).set_next_nonce(nonce + 1).await?;

        let (secret, nullifier) = self.keychain.secrets(pool, nonce).await?;
        Ok((nonce, secret, nullifier))
    }

    /// Syncs `pool`, then walks the nonces against its state to find the next reservation.
    ///
    /// Callers must hold `nonce_lock`, since this reads and writes the reservation counter.
    async fn recover(&self, pool: &Pool) -> Result<u64, WalletError> {
        self.provider.register(pool).await;
        self.provider.sync().await?;

        let store = self.pool_store(pool);
        let reserved = store.next_nonce().await?;

        let mut next = reserved;
        let mut nonce = reserved;
        let mut misses = 0;
        while misses < self.gap_limit {
            let commitment = self.keychain.note(pool, nonce).await?.commitment();
            if self.provider.commitment(pool, commitment).await?.is_some() {
                next = nonce + 1;
                misses = 0;
            } else {
                misses += 1;
            }

            nonce += 1;
        }

        if next > reserved {
            store.set_next_nonce(next).await?;
        }

        Ok(next)
    }

    /// Resolves a note for `pool` at `nonce`.
    async fn note_at(&self, pool: &Pool, nonce: u64) -> Result<WalletNote, WalletError> {
        let note = self.keychain.note(pool, nonce).await?;
        let status = self.note_status(pool, &note).await?;

        Ok(WalletNote {
            nonce,
            note,
            status,
        })
    }

    /// Resolves the status of a note for `pool` at `nonce`.
    async fn note_status(&self, pool: &Pool, note: &Note) -> Result<NoteStatus, WalletError> {
        let Some(leaf_index) = self.provider.commitment(pool, note.commitment()).await? else {
            return Ok(NoteStatus::Pending);
        };

        let spent = self
            .provider
            .is_nullifier_spent(pool, note.nullifier_hash().into())
            .await?;

        match spent {
            true => Ok(NoteStatus::Withdrawn { leaf_index }),
            false => Ok(NoteStatus::Deposited { leaf_index }),
        }
    }

    /// Returns the store scope holding `pool`'s nonce state.
    fn pool_store(&self, pool: &Pool) -> Store {
        self.store.scope(pool.id())
    }
}
