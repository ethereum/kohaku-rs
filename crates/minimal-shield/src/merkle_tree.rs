use kohaku_merkle_tree::{hasher::Hasher, MerkleTree};
use ruint::aliases::U256;

use crate::crypto::p2;

pub const DEPTH: usize = 20;
pub type MsMerkleTree = MerkleTree<DEPTH, PoseidonHasher>;

#[derive(Copy, Clone)]
pub struct PoseidonHasher;

impl Hasher for PoseidonHasher {
    fn hash(a: U256, b: U256) -> U256 {
        p2(a, b)
    }

    fn zero() -> U256 {
        U256::ZERO
    }
}

pub const EMPTY_ROOT: U256 =
    ruint::uint!(0x2134e76ac5d21aab186c2be1dd8f84ee880a1e46eaf712f9d371b6df22191f3e_U256);
