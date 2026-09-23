use alloy::primitives::{keccak256, Address};
use rand::{CryptoRng, RngExt};
use ruint::aliases::U256;
use serde::{Deserialize, Serialize};

use crate::crypto::{p2, tagged, P, TAG_LEAF, TAG_OCCURRENCE_NULL, TAG_PK};

/// `keccak256("minimal-shielded-pool:occurrence-domain:v1")`.
pub const DOMAIN_TAG: [u8; 32] = [
    0xa9, 0xd0, 0x3f, 0xa1, 0xcd, 0x97, 0xbc, 0xf3, 0x29, 0x4d, 0xc8, 0xe3, 0xbb, 0x02, 0x4f, 0x55,
    0x53, 0x93, 0xc9, 0x89, 0x67, 0xb3, 0x56, 0x96, 0x7f, 0xa5, 0x02, 0xab, 0xab, 0x36, 0x6e, 0xd3,
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub spend_key: U256,
    pub rho: U256,
    pub value: U256,
    pub chain_id: u64,
    pub pool: Address,
}

impl Note {
    pub fn random(value: U256, chain_id: u64, pool: Address, rng: &mut impl CryptoRng) -> Self {
        Self {
            spend_key: random_fe(rng),
            rho: random_fe(rng),
            value,
            chain_id,
            pool,
        }
    }

    #[must_use]
    pub fn owner_pk(&self) -> U256 {
        tagged(TAG_PK, self.spend_key, U256::ZERO)
    }

    #[must_use]
    pub fn inner(&self) -> U256 {
        p2(self.owner_pk(), self.rho)
    }

    #[must_use]
    pub fn commitment(&self) -> U256 {
        tagged(TAG_LEAF, self.inner(), self.value)
    }

    #[must_use]
    pub fn domain(&self, epoch: u64) -> U256 {
        domain_scalar(self.chain_id, self.pool, epoch)
    }

    /// Occurrence nullifier: `Poseidon(4, Poseidon2(domain, spend_key), Poseidon2(cm, index))`.
    #[must_use]
    pub fn nullifier(&self, domain: U256, index: U256) -> U256 {
        tagged(
            TAG_OCCURRENCE_NULL,
            p2(domain, self.spend_key),
            p2(self.commitment(), index),
        )
    }
}

/// `keccak256(DOMAIN_TAG || chain_id || pool || epoch) mod Fr`, matching `ShieldedPoolLogic.domainFor`.
#[must_use]
pub fn domain_scalar(chain_id: u64, pool: Address, epoch: u64) -> U256 {
    let mut buf = [0u8; 128];
    buf[..32].copy_from_slice(&DOMAIN_TAG);
    buf[32..64].copy_from_slice(&U256::from(chain_id).to_be_bytes::<32>());
    buf[76..96].copy_from_slice(pool.as_slice());
    buf[96..128].copy_from_slice(&U256::from(epoch).to_be_bytes::<32>());
    let hash = keccak256(buf);
    U256::from_be_bytes(hash.0) % P
}

fn random_fe(rng: &mut impl CryptoRng) -> U256 {
    loop {
        let b: [u8; 32] = rng.random();
        let n = U256::from_be_bytes(b);
        if n < P && !n.is_zero() {
            return n;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_tag_is_occurrence_v1() {
        assert_eq!(
            DOMAIN_TAG,
            keccak256(b"minimal-shielded-pool:occurrence-domain:v1").0
        );
    }

    #[test]
    fn domain_includes_epoch() {
        let pool = Address::repeat_byte(0x11);
        assert_ne!(domain_scalar(8141, pool, 0), domain_scalar(8141, pool, 1));
    }

    #[test]
    fn domain_matches_position_notes_v1_pool() {
        let pool: Address = "0xac01c30f28b32dd31d3c2854012e673e74f6b100"
            .parse()
            .unwrap();
        let want = U256::from_str_radix(
            "299ba2eaf9e8f65969c2421e1d0a34955b3870ea45e6dddabd4463e4c3c50778",
            16,
        )
        .unwrap();
        assert_eq!(domain_scalar(8141, pool, 0), want);
    }
}
