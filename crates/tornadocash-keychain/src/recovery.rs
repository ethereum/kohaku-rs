use std::collections::HashMap;

use kohaku_tornadocash::{
    pool::Pool,
    syncer::event::{self, SyncEvent},
};
use ruint::aliases::U256;

use crate::keychain::{Keychain, KeychainError};

/// Number of consecutive nonces without a deposit after which a scan gives up.
const DEFAULT_GAP_LIMIT: u64 = 20;

/// A note recovered from a pool's synced events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveredNote {
    pub pool: Pool,
    /// The nonce used to derive the note.
    pub nonce: u64,
    /// The deposit event that created the note.
    pub deposit: event::Deposit,
    /// The withdrawal event that spent the note, if any.
    pub withdrawal: Option<event::Withdrawal>,
}

/// Recovers every note `keychain` derived for `pool` that appears in `events`.
///
/// Incrementally scans nonces starting from 0, finding deposits and withdrawals in `events` that
/// match each derived note. Scanning stops after `gap_limit` consecutive nonces with no deposits.
///
/// `events` should be a complete list of all deposits and withdrawals for `pool` up to the current
/// block. If events is incomplete, some notes may be missed.
///
/// # Errors
/// Returns an error if the keychain cannot derive a nonce's material.
pub async fn recover(
    keychain: &impl Keychain,
    pool: &Pool,
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
    }

    Ok(recovered)
}

/// Indexes the deposits in `events` by commitment.
fn deposits(events: &[SyncEvent]) -> HashMap<U256, &event::Deposit> {
    events
        .iter()
        .filter_map(|event| match event {
            SyncEvent::Deposit(deposit) => Some((deposit.commitment.into(), deposit)),
            SyncEvent::Withdrawal(_) => None,
        })
        .collect()
}

/// Indexes the withdrawals in `events` by nullifier hash.
fn withdrawals(events: &[SyncEvent]) -> HashMap<U256, &event::Withdrawal> {
    events
        .iter()
        .filter_map(|event| match event {
            SyncEvent::Withdrawal(withdrawal) => {
                Some((withdrawal.nullifier_hash.into(), withdrawal))
            }
            SyncEvent::Deposit(_) => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use alloy::primitives::Address;
    use kohaku_tornadocash::{
        Note,
        note::{Nullifier, Secret},
    };

    use super::*;

    const POOL: Pool = Pool::ETHEREUM_ETHER_01;

    /// A keychain deriving both secrets from the nonce alone.
    struct TestKeychain;

    #[tokio::test]
    async fn test_recovers_deposited_notes() {
        let events = vec![
            deposit(&note(0).await, 0),
            deposit(&foreign_note(), 1),
            deposit(&note(1).await, 2),
        ];

        let recovered = recover(&TestKeychain, &POOL, &events, Some(2))
            .await
            .unwrap();

        assert_eq!(recovered.len(), 2);
        assert_eq!(recovered[0].nonce, 0);
        assert_eq!(recovered[0].deposit.leaf_index, 0);
        assert_eq!(recovered[1].nonce, 1);
        assert_eq!(recovered[1].deposit.leaf_index, 2);
    }

    #[tokio::test]
    async fn test_recovers_withdrawals() {
        let events = vec![
            deposit(&note(0).await, 0),
            deposit(&note(1).await, 1),
            withdrawal(&note(1).await),
            withdrawal(&foreign_note()),
        ];

        let recovered = recover(&TestKeychain, &POOL, &events, Some(2))
            .await
            .unwrap();

        assert_eq!(recovered.len(), 2);
        assert_eq!(recovered[0].nonce, 0);
        assert_eq!(recovered[0].withdrawal, None);
        assert_eq!(recovered[1].nonce, 1);
        assert!(recovered[1].withdrawal.is_some());
    }

    #[tokio::test]
    async fn test_stops_at_the_gap_limit() {
        let events = vec![deposit(&note(0).await, 0), deposit(&note(3).await, 1)];

        let recovered = recover(&TestKeychain, &POOL, &events, Some(2))
            .await
            .unwrap();

        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].nonce, 0);
    }

    #[async_trait::async_trait]
    impl Keychain for TestKeychain {
        async fn secrets(
            &self,
            _pool: &Pool,
            nonce: u64,
        ) -> Result<(Secret, Nullifier), KeychainError> {
            let mut bytes = [0u8; 31];
            bytes[..8].copy_from_slice(&nonce.to_be_bytes());

            Ok((Secret::new(bytes), Nullifier::new(bytes)))
        }
    }

    async fn note(nonce: u64) -> Note {
        let (secret, nullifier) = TestKeychain.secrets(&POOL, nonce).await.unwrap();
        Note::new(nullifier, secret)
    }

    fn foreign_note() -> Note {
        Note::new([9u8; 31], [9u8; 31])
    }

    fn deposit(note: &Note, leaf_index: u32) -> SyncEvent {
        SyncEvent::Deposit(event::Deposit {
            commitment: note.commitment().into(),
            leaf_index,
            block_number: 0,
        })
    }

    fn withdrawal(note: &Note) -> SyncEvent {
        SyncEvent::Withdrawal(event::Withdrawal {
            to: Address::ZERO,
            nullifier_hash: note.nullifier_hash().into(),
            relayer: Address::ZERO,
            fee: U256::ZERO,
            block_number: 0,
        })
    }
}
