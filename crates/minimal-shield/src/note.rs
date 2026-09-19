use alloy::primitives::{Address, keccak256};
use rand::{CryptoRng, RngExt};
use ruint::aliases::U256;
use serde::{Deserialize, Serialize};

use crate::crypto::{P, TAG_LEAF, TAG_NULL, TAG_PK, p2, tagged};

pub const DOMAIN_TAG: [u8; 32] = [
    0x40, 0x75, 0x2e, 0x10, 0x2d, 0x2a, 0x74, 0x9c, 0x61, 0xd4, 0x2a, 0x71, 0xe2, 0x97, 0xed, 0xd3,
    0xb4, 0x93, 0xde, 0x63, 0x90, 0x03, 0xb9, 0x48, 0x0a, 0x70, 0x0d, 0x58, 0x9d, 0x98, 0x06, 0x5b,
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
    pub fn domain(&self) -> U256 {
        domain_scalar(self.chain_id, self.pool)
    }

    #[must_use]
    pub fn nullifier(&self) -> U256 {
        tagged(TAG_NULL, p2(self.domain(), self.spend_key), self.commitment())
    }
}

#[must_use]
pub fn domain_scalar(chain_id: u64, pool: Address) -> U256 {
    let mut buf = [0u8; 96];
    buf[..32].copy_from_slice(&DOMAIN_TAG);
    buf[32..64].copy_from_slice(&U256::from(chain_id).to_be_bytes::<32>());
    buf[76..96].copy_from_slice(pool.as_slice());
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
