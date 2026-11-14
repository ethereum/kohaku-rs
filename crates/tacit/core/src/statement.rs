//! The statement one `transact` proves: two inputs (owned notes or zero-value fillers), two
//! outputs, and the value crossing the pool boundary.

use alloy::primitives::{Address, Bytes, I256, U256 as AlloyU256, keccak256};
use ark_ff::PrimeField;
use kohaku_tacit_circuit::CircuitInputs;
use rand_core::CryptoRng;
use ruint::aliases::{U256, U512};

use crate::{
    babyjubjub::mul_base8,
    field::{Fr, Scalar, fr_mod, scalar_word, word},
    keys::{KeyError, VALUE_LIMIT, leaf_of, npk_of, nullifier_of, sign},
    poseidon::poseidon,
    tree::{IncTree, TREE_DEPTH, TreeError, root_after_insert, root_from_path},
};

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StatementError {
    #[error(transparent)]
    Key(#[from] KeyError),
    #[error(transparent)]
    Tree(#[from] TreeError),
    #[error("the pool's size must be even")]
    OddSize,
    #[error("input {0} is not the leaf at its index")]
    NotTheLeaf(u64),
    #[error("value is not conserved")]
    NotConserved,
    #[error("both inputs have the same nullifier")]
    DuplicateNullifier,
    #[error("external amount or fee out of range")]
    AmountOutOfRange,
}

/// The proof's `asset` input: `keccak256(abi.encode(chainId, pool, token)) mod p`, the zero token
/// for ETH.
#[must_use]
pub fn pool_asset(chain_id: u64, pool: Address, token: Address) -> U256 {
    word(asset_fr(chain_id, pool, token))
}

pub(crate) fn asset_fr(chain_id: u64, pool: Address, token: Address) -> Fr {
    let mut data = Vec::with_capacity(96);
    data.extend_from_slice(&AlloyU256::from(chain_id).to_be_bytes::<32>());
    data.extend_from_slice(pool.into_word().as_slice());
    data.extend_from_slice(token.into_word().as_slice());
    fr_mod(U256::from_be_bytes(keccak256(&data).0))
}

/// Everything a proof binds that the circuit does not compute: where value goes, who relays it and
/// for what fee, and the output memos. The pool recomputes [`ExtData::hash`] from its call
/// arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtData {
    pub chain_id: u64,
    pub pool: Address,
    pub recipient: Address,
    /// Positive into the pool, negative out of it to `recipient`.
    pub ext_amount: i128,
    pub relayer: Address,
    pub fee: u128,
    pub memo0: Bytes,
    pub memo1: Bytes,
}

impl ExtData {
    /// `keccak256(abi.encode(chainId, pool, recipient, extAmount, relayer, fee, keccak256(memo0),
    /// keccak256(memo1))) mod p`.
    ///
    /// # Errors
    /// Returns an error if the amount or fee is outside what the circuit can hold.
    pub fn hash(&self) -> Result<U256, StatementError> {
        Ok(word(self.hash_fr()?))
    }

    pub(crate) fn hash_fr(&self) -> Result<Fr, StatementError> {
        let limit = VALUE_LIMIT.cast_signed();
        if self.ext_amount <= -limit || self.ext_amount >= limit || self.fee >= VALUE_LIMIT {
            return Err(StatementError::AmountOutOfRange);
        }
        let mut data = Vec::with_capacity(256);
        data.extend_from_slice(&AlloyU256::from(self.chain_id).to_be_bytes::<32>());
        data.extend_from_slice(self.pool.into_word().as_slice());
        data.extend_from_slice(self.recipient.into_word().as_slice());
        data.extend_from_slice(&i256(self.ext_amount).to_be_bytes::<32>());
        data.extend_from_slice(self.relayer.into_word().as_slice());
        data.extend_from_slice(&AlloyU256::from(self.fee).to_be_bytes::<32>());
        data.extend_from_slice(keccak256(&self.memo0).as_slice());
        data.extend_from_slice(keccak256(&self.memo1).as_slice());
        Ok(fr_mod(U256::from_be_bytes(keccak256(&data).0)))
    }
}

/// `x` as an `int256`.
pub(crate) fn i256(x: i128) -> I256 {
    let magnitude = I256::from_raw(AlloyU256::from(x.unsigned_abs()));
    if x < 0 { -magnitude } else { magnitude }
}

/// `extAmount − fee mod p`.
pub(crate) fn public_amount(ext_amount: i128, fee: u128) -> Fr {
    let fee = i128::try_from(fee).expect("fee below 2^120");
    let x = ext_amount - fee;
    let magnitude = Fr::from(x.unsigned_abs());
    if x < 0 { -magnitude } else { magnitude }
}

/// One input slot.
#[derive(Clone, Copy)]
pub(crate) enum SpendInput {
    /// An owned note at `index`.
    Note {
        value: u128,
        rho: Fr,
        nk: Scalar,
        sk: Scalar,
        index: u64,
    },
    /// A zero-value filler with fresh random keys.
    Filler,
}

/// One output slot: a new note, or nothing (a transaction with no outputs inserts nothing).
#[derive(Clone, Copy)]
pub(crate) struct OutputNote {
    pub value: u128,
    pub npk: Fr,
    pub rho: Fr,
}

/// A built statement: the circuit inputs and what the pool will record.
#[derive(Clone, Debug)]
pub(crate) struct Statement {
    pub inputs: CircuitInputs,
    pub nullifiers: [Fr; 2],
    pub out_leaves: [Fr; 2],
}

struct Slot {
    value: u128,
    rho: Fr,
    nk: Scalar,
    sk: Scalar,
    index: u64,
    path: [Fr; TREE_DEPTH],
    nf: Fr,
}

fn random_bytes<const N: usize>(rng: &mut impl CryptoRng) -> [u8; N] {
    let mut out = [0u8; N];
    rng.fill_bytes(&mut out);
    out
}

/// A non-zero scalar `1 + (x mod (L − 1))` for 512 random bits `x`.
fn filler_scalar(rng: &mut impl CryptoRng) -> Scalar {
    let x = U512::from_be_bytes(random_bytes::<64>(rng));
    let l1 = U512::from(U256::from_limbs(Scalar::MODULUS.0) - U256::from(1));
    let k = U256::from(x % l1) + U256::from(1);
    Scalar::from_bigint(ark_ff::BigInt(k.into_limbs())).expect("below L")
}

fn slot(
    input: SpendInput,
    asset: Fr,
    tree: &IncTree,
    rng: &mut impl CryptoRng,
) -> Result<Slot, StatementError> {
    match input {
        SpendInput::Filler => {
            let sk = filler_scalar(rng);
            let nk = filler_scalar(rng);
            let rho = Fr::from_be_bytes_mod_order(&random_bytes::<64>(rng));
            let index = u64::from(u32::from_be_bytes(random_bytes::<4>(rng)));
            let leaf = leaf_of(asset, 0, npk_of(mul_base8(sk), mul_base8(nk)), rho)?;
            Ok(Slot {
                value: 0,
                rho,
                nk,
                sk,
                index,
                path: [Fr::from(0u8); TREE_DEPTH],
                nf: nullifier_of(nk, leaf, index)?,
            })
        }
        SpendInput::Note {
            value,
            rho,
            nk,
            sk,
            index,
        } => {
            let leaf = leaf_of(asset, value, npk_of(mul_base8(sk), mul_base8(nk)), rho)?;
            let path = tree.path(index)?;
            if root_from_path(leaf, index, &path) != tree.root_fr() {
                return Err(StatementError::NotTheLeaf(index));
            }
            Ok(Slot {
                value,
                rho,
                nk,
                sk,
                index,
                path,
                nf: nullifier_of(nk, leaf, index)?,
            })
        }
    }
}

/// Builds the statement for one `transact`. Membership is proven against `tree`, and the outputs
/// are inserted at its current size, so the proof holds only while the pool has that head.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build(
    asset: Fr,
    tree: &IncTree,
    inputs: [SpendInput; 2],
    outputs: [Option<OutputNote>; 2],
    ext_amount: i128,
    fee: u128,
    ext_data_hash: Fr,
    rng: &mut impl CryptoRng,
) -> Result<Statement, StatementError> {
    if tree.size() % 2 == 1 {
        return Err(StatementError::OddSize);
    }
    let public_amount = public_amount(ext_amount, fee);
    let ins = [
        slot(inputs[0], asset, tree, rng)?,
        slot(inputs[1], asset, tree, rng)?,
    ];

    let outs = outputs.map(|o| {
        o.unwrap_or(OutputNote {
            value: 0,
            npk: Fr::from(0u8),
            rho: Fr::from(0u8),
        })
    });
    let mut out_leaves = [Fr::from(0u8); 2];
    for (k, output) in outputs.iter().enumerate() {
        if let Some(o) = output {
            out_leaves[k] = leaf_of(asset, o.value, o.npk, o.rho)?;
        }
    }

    let fee_signed = i128::try_from(fee).map_err(|_| StatementError::AmountOutOfRange)?;
    let value_in: i128 =
        ins.iter().map(|s| s.value.cast_signed()).sum::<i128>() + ext_amount - fee_signed;
    let value_out: i128 = outs.iter().map(|o| o.value.cast_signed()).sum();
    if value_in != value_out {
        return Err(StatementError::NotConserved);
    }
    let nullifiers = [ins[0].nf, ins[1].nf];
    if nullifiers[0] == nullifiers[1] {
        return Err(StatementError::DuplicateNullifier);
    }

    let message = poseidon(&[
        asset,
        nullifiers[0],
        nullifiers[1],
        out_leaves[0],
        out_leaves[1],
        public_amount,
        ext_data_hash,
    ]);
    let sigs = ins.each_ref().map(|s| sign(s.sk, message));
    let ins_path = tree.insertion_path();
    let new_root = root_after_insert(out_leaves, tree.size(), &ins_path);
    let root = word(tree.root_fr());

    let point = |p: crate::babyjubjub::Point| [word(p.x), word(p.y)];
    let inputs = CircuitInputs {
        root,
        old_root: root,
        new_root: word(new_root),
        start_index: U256::from(tree.size()),
        public_amount: word(public_amount),
        ext_data_hash: word(ext_data_hash),
        asset: word(asset),
        nf: nullifiers.map(word),
        out_leaf: out_leaves.map(word),
        in_v: ins.each_ref().map(|s| U256::from(s.value)),
        in_rho: ins.each_ref().map(|s| word(s.rho)),
        in_nk: ins.each_ref().map(|s| scalar_word(s.nk)),
        in_ak: ins.each_ref().map(|s| point(mul_base8(s.sk))),
        in_index: ins.each_ref().map(|s| U256::from(s.index)),
        in_path: ins.each_ref().map(|s| s.path.map(word)),
        sig_r8: sigs.map(|s| point(s.r8)),
        sig_s: sigs.map(|s| scalar_word(s.s)),
        out_v: outs.map(|o| U256::from(o.value)),
        out_npk: outs.map(|o| word(o.npk)),
        out_rho: outs.map(|o| word(o.rho)),
        ins_path: ins_path.map(word),
    };
    Ok(Statement {
        inputs,
        nullifiers,
        out_leaves,
    })
}
