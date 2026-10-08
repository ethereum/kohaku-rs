use std::collections::HashMap;

use kohaku_tornadocash::{
    Field, Pool,
    syncer::{Deposit, SyncEvent, Withdrawal},
};

use crate::{Keychain, KeychainError};

/// Number of consecutive nonces without a deposit after which a scan gives up.
const DEFAULT_GAP_LIMIT: u64 = 20;

/// A note recovered from a pool's synced events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveredNote {
    pub pool: Pool,
    /// The nonce used to derive the note.
    pub nonce: u64,
    /// The deposit event that created the note.
    pub deposit: Deposit,
    /// The withdrawal event that spent the note, if any.
    pub withdrawal: Option<Withdrawal>,
}

/// Returns the next nonce that should be used for a new note derived from `keychain` for `pool`.
///
/// See [`recover`] for details on how the next nonce is determined.
pub async fn next_nonce(
    keychain: &impl Keychain,
    pools: &[Pool],
    events: &[SyncEvent],
    gap_limit: Option<u64>,
) -> Result<u64, KeychainError> {
    let recovered = recover(keychain, pools, events, gap_limit).await?;
    Ok(recovered.last().map_or(0, |note| note.nonce + 1))
}

/// Recovers every note `keychain` derived for `pool` that appears in `events`.
///
/// Incrementally scans nonces starting from 0, finding deposits and withdrawals in `events` that
/// match each derived note. Scanning stops after `gap_limit` consecutive nonces with no deposits.
///
/// Recovery may be incomplete if `events` does not contain all deposits and withdrawals made by
/// `keychain` for `pool`. In particular, if a deposit was previously created but not yet submitted
/// on-chain it becomes a racy deposit. If its nonce is handed out again then both deposits derive
/// the same commitment, so only one can be submitted. Broadcasting them from different senders
/// also links those addresses, since the losing transaction is still public.
///
/// # Errors
/// Returns an error if the keychain cannot derive a nonce's material.
pub async fn recover(
    keychain: &impl Keychain,
    pools: &[Pool],
    events: &[SyncEvent],
    gap_limit: Option<u64>,
) -> Result<Vec<RecoveredNote>, KeychainError> {
    let gap_limit = gap_limit.unwrap_or(DEFAULT_GAP_LIMIT);
    if gap_limit == 0 {
        return Ok(Vec::new());
    }

    let deposits = deposits(events);
    let withdrawals = withdrawals(events);

    let mut recovered = Vec::new();
    let mut misses = 0;
    for nonce in 0.. {
        if misses >= gap_limit {
            break;
        }
        misses += 1;

        for pool in pools {
            let commitment = keychain.commitment(pool, nonce).await?;
            let Some(deposit) = deposits.get(&commitment).copied() else {
                continue;
            };

            misses = 0;

            let nullifier_hash = keychain.nullifier_hash(pool, nonce).await?;
            recovered.push(RecoveredNote {
                pool: pool.clone(),
                nonce,
                deposit: deposit.clone(),
                withdrawal: withdrawals.get(&nullifier_hash).copied().cloned(),
            });
            break;
        }
    }

    Ok(recovered)
}

/// Indexes the deposits in `events` by commitment.
fn deposits(events: &[SyncEvent]) -> HashMap<Field, &Deposit> {
    events
        .iter()
        .filter_map(|event| match event {
            SyncEvent::Deposit(deposit) => Some((deposit.commitment, deposit)),
            SyncEvent::Withdrawal(_) => None,
        })
        .collect()
}

/// Indexes the withdrawals in `events` by nullifier hash.
fn withdrawals(events: &[SyncEvent]) -> HashMap<Field, &Withdrawal> {
    events
        .iter()
        .filter_map(|event| match event {
            SyncEvent::Withdrawal(withdrawal) => Some((withdrawal.nullifier_hash, withdrawal)),
            SyncEvent::Deposit(_) => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use alloy::primitives::{Address, U256};
    use kohaku_tornadocash::{Note, NoteString, Nullifier, Secret};

    use super::*;

    const POOL: Pool = Pool::ETHEREUM_ETHER_01;
    const POOL2: Pool = Pool::ETHEREUM_ETHER_10;

    struct TestKeychain;

    #[tokio::test]
    async fn recovers_deposits() {
        let events = vec![
            deposit(&note(&POOL, 0).await, 0),
            deposit(&note(&POOL2, 1).await, 1),
            deposit(&note(&POOL, 1).await, 2),
        ];

        let recovered = recover(&TestKeychain, &[POOL], &events, Some(2))
            .await
            .unwrap();

        assert_eq!(recovered.len(), 2);
        assert_eq!(recovered[0].nonce, 0);
        assert_eq!(recovered[0].deposit.leaf_index, 0);
        assert_eq!(recovered[1].nonce, 1);
        assert_eq!(recovered[1].deposit.leaf_index, 2);
    }

    #[tokio::test]
    async fn recovers_deposits_from_multiple_pools() {
        let events = vec![
            deposit(&note(&POOL, 0).await, 0),
            deposit(&note(&POOL2, 1).await, 1),
            deposit(&note(&POOL, 2).await, 2),
        ];

        let recovered = recover(&TestKeychain, &[POOL, POOL2], &events, Some(2))
            .await
            .unwrap();

        assert_eq!(recovered.len(), 3);
        assert_eq!(recovered[0].nonce, 0);
        assert_eq!(recovered[0].deposit.leaf_index, 0);
        assert_eq!(recovered[1].nonce, 1);
        assert_eq!(recovered[1].deposit.leaf_index, 1);
        assert_eq!(recovered[2].nonce, 2);
        assert_eq!(recovered[2].deposit.leaf_index, 2);
    }

    #[tokio::test]
    async fn recovers_withdrawals() {
        let events = vec![
            deposit(&note(&POOL, 0).await, 0),
            deposit(&note(&POOL, 1).await, 1),
            withdrawal(&note(&POOL, 1).await),
            withdrawal(&note(&POOL2, 3).await),
        ];

        let recovered = recover(&TestKeychain, &[POOL], &events, Some(2))
            .await
            .unwrap();

        assert_eq!(recovered.len(), 2);
        assert_eq!(recovered[0].nonce, 0);
        assert_eq!(recovered[0].withdrawal, None);
        assert_eq!(recovered[1].nonce, 1);
        assert!(recovered[1].withdrawal.is_some());
    }

    #[tokio::test]
    async fn stops_at_gap_limit() {
        let events = vec![
            deposit(&note(&POOL, 0).await, 0),
            deposit(&note(&POOL, 3).await, 1),
        ];

        let recovered = recover(&TestKeychain, &[POOL], &events, Some(2))
            .await
            .unwrap();

        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].nonce, 0);
    }

    #[tokio::test]
    async fn next_nonce_is_incremented() {
        let events = vec![
            deposit(&note(&POOL, 0).await, 0),
            deposit(&note(&POOL, 1).await, 1),
            withdrawal(&note(&POOL, 1).await),
        ];

        let next_nonce = next_nonce(&TestKeychain, &[POOL], &events, None)
            .await
            .unwrap();
        assert_eq!(next_nonce, 2);
    }

    #[async_trait::async_trait]
    impl Keychain for TestKeychain {
        async fn note(&self, pool: &Pool, nonce: u64) -> Result<NoteString, KeychainError> {
            let mut bytes = [0u8; 31];
            bytes[..8].copy_from_slice(&nonce.to_be_bytes());
            bytes[8..16].copy_from_slice(&pool.chain_id.to_be_bytes());
            bytes[16..26].copy_from_slice(&pool.address.to_vec()[..10]);

            Ok(NoteString::from_pool(
                Note::new(Nullifier::new(bytes), Secret::new(bytes)),
                pool,
            ))
        }
    }

    async fn note(pool: &Pool, nonce: u64) -> Note {
        TestKeychain.note(pool, nonce).await.unwrap().note
    }

    fn deposit(note: &Note, leaf_index: u32) -> SyncEvent {
        SyncEvent::Deposit(Deposit {
            commitment: note.commitment(),
            leaf_index,
            block_number: 0,
        })
    }

    fn withdrawal(note: &Note) -> SyncEvent {
        SyncEvent::Withdrawal(Withdrawal {
            to: Address::ZERO,
            nullifier_hash: note.nullifier_hash(),
            relayer: Address::ZERO,
            fee: U256::ZERO,
            block_number: 0,
        })
    }
}
