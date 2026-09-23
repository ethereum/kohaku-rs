use kohaku_kv_store::{Store, backend::StoreError};
use ruint::aliases::U256;

const LATEST_BLOCK_KEY: &[u8] = b"latest_block";
const COMMITMENT_PREFIX: &[u8] = b"commitment";
const NULLIFIER_HASH_PREFIX: &[u8] = b"nullifier";

#[async_trait::async_trait]
pub trait IndexerStoreExt {
    async fn latest_block(&self) -> Result<u64, StoreError>;
    /// Returns the `leaf_index` of the given `commitment` if it exists in the store.
    async fn get_commitment(&self, commitment: U256) -> Result<Option<u32>, StoreError>;
    /// Returns `Some` if the given `nullifier_hash` exists in the store.
    async fn get_nullifier_hash(&self, nullifier_hash: U256) -> Result<Option<()>, StoreError>;
    /// Commits the given `latest_block`, `commitments`, and `nullifier_hashes` to the store.
    async fn commit(
        &self,
        latest_block: u64,
        commitments: &[(u32, U256)],
        nullifier_hashes: &[U256],
    ) -> Result<(), StoreError>;
}

#[async_trait::async_trait]
impl IndexerStoreExt for Store {
    async fn latest_block(&self) -> Result<u64, StoreError> {
        Ok(self.get(LATEST_BLOCK_KEY).await?.map_or(0, |v| {
            u64::from_be_bytes(v.try_into().expect("latest block is 8 bytes"))
        }))
    }

    async fn get_commitment(&self, commitment: U256) -> Result<Option<u32>, StoreError> {
        let key = commitment_key(commitment);
        Ok(self
            .get(&key)
            .await?
            .map(|v| u32::from_be_bytes(v.try_into().expect("commitment is 4 bytes"))))
    }

    async fn get_nullifier_hash(&self, nullifier_hash: U256) -> Result<Option<()>, StoreError> {
        let key = nullifier_hash_key(nullifier_hash);
        Ok(self.get(&key).await?.map(|_| ()))
    }

    async fn commit(
        &self,
        latest_block: u64,
        commitments: &[(u32, U256)],
        nullifier_hashes: &[U256],
    ) -> Result<(), StoreError> {
        let mut batch = self.batch();
        batch.put(LATEST_BLOCK_KEY, latest_block.to_be_bytes());

        for (leaf_index, commitment) in commitments {
            batch.put(commitment_key(*commitment), leaf_index.to_be_bytes());
        }

        for nullifier_hash in nullifier_hashes {
            batch.put(nullifier_hash_key(*nullifier_hash), &[]);
        }

        batch.commit().await
    }
}

fn commitment_key(commitment: U256) -> Vec<u8> {
    let mut key = COMMITMENT_PREFIX.to_vec();
    key.extend_from_slice(&commitment.to_be_bytes_vec());
    key
}

fn nullifier_hash_key(nullifier_hash: U256) -> Vec<u8> {
    let mut key = NULLIFIER_HASH_PREFIX.to_vec();
    key.extend_from_slice(&nullifier_hash.to_be_bytes_vec());
    key
}
