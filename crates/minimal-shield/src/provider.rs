use alloy::{
    primitives::{Address, B256, Bytes, U256 as AlloyU256, keccak256},
    signers::{SignerSync, local::PrivateKeySigner},
    sol_types::{SolCall, SolValue},
};
use kohaku_frametx_kit::{
    ACTION_FRAME_MAX_CALLDATA, ACTION_FRAME_MAX_GAS, ACTION_FRAME_MAX_STATE_GAS,
    APPROVE_EXECUTION_AND_PAYMENT, CLAIM_FRAME_GAS, CLAIM_FRAME_STATE_GAS, FRAME_MODE_DEFAULT,
    FRAME_MODE_SENDER, FRAME_MODE_VERIFY, Frame, FrameSig, FrameTx, RECENT_ROOT_ADDRESS,
    RECENT_ROOT_FRAME_GAS, SETTLE_FRAME_GAS, SETTLE_FRAME_STATE_GAS, SHIELD_VERIFY_GAS,
    VERIFY_FRAME_GAS, VERIFY_FRAME_STATE_GAS, recent_root_tuple_bytes, source_id,
};
use kohaku_minimal_shield_circuit::prove;
use ruint::aliases::U256;
use thiserror::Error;

use crate::{
    abis::{
        FrameAccount::Call as AccountCall,
        ShieldedPool::{self, Spend as SolSpend},
        UnshieldHook,
    },
    indexer::Indexer,
    note::Note,
    spend::{SpendInput, SpendWitness, sink_outputs},
};

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("circuit: {0}")]
    Circuit(String),
    #[error("signer: {0}")]
    Signer(String),
    #[error("note is not in the synced tree")]
    MissingNote,
    #[error("FrameAccount executeBatch requires the owner's signature")]
    MissingOwnerSig,
    #[error("owner signer does not match CREATE2 owner")]
    OwnerMismatch,
    #[error("note value {value} cannot cover fee {fee}")]
    FeeExceedsValue { value: U256, fee: U256 },
    #[error("leftover-frame calldata {got} exceeds dispatcher cap {cap}")]
    TailTooLarge { got: usize, cap: usize },
    #[error("leftover-frame gas {got} exceeds dispatcher cap {cap}")]
    TailGas { got: u64, cap: u64 },
    #[error("DEFAULT tail target must be nonzero")]
    ZeroTailTarget,
    #[error(transparent)]
    Frame(#[from] kohaku_frametx_kit::FrameTxError),
    #[error(transparent)]
    Indexer(#[from] crate::indexer::IndexerError),
}

/// Signed spend plus the public amount credited to `recipient`.
#[derive(Clone, Debug)]
pub struct UnshieldResult {
    pub tx: FrameTx,
    pub public_amount: U256,
    pub fee: U256,
}

#[derive(Clone)]
pub struct PoolProvider {
    pub indexer: Indexer,
}

#[derive(Debug, Clone)]
pub struct CreateAccount {
    pub owner: Address,
    pub salt: B256,
}

#[derive(Debug, Clone)]
pub struct Call {
    pub target: Address,
    pub value: AlloyU256,
    pub data: Bytes,
}

/// Optional leftover DEFAULT frame (`mode = 0`) on a 4-frame spend.
#[derive(Debug, Clone)]
pub struct TailCall {
    pub target: Address,
    pub data: Bytes,
    pub execution_gas: u64,
    pub state_gas: u64,
}

/// Fifth-frame SENDER call used by [`PoolProvider::hook_complete`].
#[derive(Debug, Clone)]
pub struct WildCall {
    pub target: Address,
    pub data: Bytes,
}

impl PoolProvider {
    #[must_use]
    pub fn new(indexer: Indexer) -> Self {
        Self { indexer }
    }

    /// User-funded shield: VERIFY(self, flags=0x03) + SENDER(pool, value, shield(inner)).
    #[must_use]
    pub fn shield(
        &self,
        note: &Note,
        sender: Address,
        nonce_seq: u64,
        chain_id: u64,
        max_priority_fee: AlloyU256,
        max_fee: AlloyU256,
    ) -> FrameTx {
        let inner = u256_to_b256(note.inner());
        let data = Bytes::from(ShieldedPool::shieldCall { inner }.abi_encode());
        FrameTx {
            chain_id,
            nonce_keys: vec![AlloyU256::ZERO],
            nonce_seq,
            sender,
            frames: vec![
                Frame {
                    mode: FRAME_MODE_VERIFY,
                    flags: APPROVE_EXECUTION_AND_PAYMENT,
                    target: Some(sender),
                    execution_gas: SHIELD_VERIFY_GAS,
                    state_gas: 0,
                    value: AlloyU256::ZERO,
                    data: Bytes::new(),
                },
                Frame {
                    mode: FRAME_MODE_SENDER,
                    flags: 0,
                    target: Some(self.indexer.pool().address),
                    execution_gas: SETTLE_FRAME_GAS,
                    state_gas: 0,
                    value: alloy_u256(note.value),
                    data,
                },
            ],
            signatures: vec![FrameSig::secp256k1(sender)],
            max_priority_fee,
            max_fee,
            max_blob_fee: AlloyU256::ZERO,
            blob_hashes: vec![],
        }
    }

    /// DEFAULT leftover that claims `who`'s credit from the pool.
    #[must_use]
    pub fn claim_tail(&self, who: Address) -> TailCall {
        TailCall {
            target: self.indexer.pool().address,
            data: Bytes::from(ShieldedPool::claimWithdrawalCall { who }.abi_encode()),
            execution_gas: CLAIM_FRAME_GAS,
            state_gas: CLAIM_FRAME_STATE_GAS,
        }
    }

    /// Three- or four-frame pool-as-sender spend. Frame 3 is a generic DEFAULT tail.
    ///
    /// `fee` of `None` uses [`FrameTx::max_cost`] (dispatcher `fee >= TXPARAM(0x06)`).
    ///
    /// # Errors
    /// Returns if the note is missing, the tail is over cap, the note cannot cover
    /// the fee, or proving fails.
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    pub async fn unshield(
        &self,
        note: &Note,
        dummy: &Note,
        recipient: Address,
        tail: Option<TailCall>,
        fee: Option<U256>,
        authorizer: &PrivateKeySigner,
        root_slot: u64,
        epoch: u64,
        chain_id: u64,
        max_priority_fee: AlloyU256,
        max_fee: AlloyU256,
    ) -> Result<UnshieldResult, ProviderError> {
        if let Some(t) = &tail {
            validate_tail(t)?;
        }
        self.indexer.sync().await?;
        let tree = self.indexer.tree();
        let proof = tree
            .leaf_proof(note.commitment())
            .await
            .map_err(|_| ProviderError::MissingNote)?;
        let mut siblings = [U256::ZERO; 20];
        let mut bits = [U256::ZERO; 20];
        siblings.copy_from_slice(&proof.siblings);
        for (i, b) in proof.path.iter().enumerate() {
            bits[i] = U256::from(*b);
        }

        let mut dummy_note = dummy.clone();
        dummy_note.value = U256::ZERO;
        dummy_note.chain_id = note.chain_id;
        dummy_note.pool = note.pool;

        let sinks = sink_outputs();
        let mut witness = SpendWitness {
            inputs: [
                SpendInput {
                    note: Some(note.clone()),
                    siblings,
                    bits,
                },
                SpendInput::dummy(dummy_note),
            ],
            out_inner: [sinks[0].0, sinks[1].0],
            out_value: [sinks[0].1, sinks[1].1],
            public_amount: note.value,
            fee: U256::ZERO,
            recipient,
            authorizer: authorizer.address(),
            root: proof.root,
            domain: note.domain(),
        };

        let nfs = witness.nullifiers();
        let outs = witness.output_commitments();
        let mut keys = [alloy_u256(nfs[0]), alloy_u256(nfs[1])];
        keys.sort();
        let src = source_id(self.indexer.pool().address, epoch);
        let tuple = recent_root_tuple_bytes(src, root_slot, u256_to_b256(witness.root));
        let dummy_proof = Bytes::from(vec![1u8; 256]);

        let mut chosen_fee = fee.unwrap_or(U256::ZERO);
        for _ in 0..2 {
            let public_amount = note.value.saturating_sub(chosen_fee);
            if public_amount.is_zero() && !note.value.is_zero() && chosen_fee >= note.value {
                return Err(ProviderError::FeeExceedsValue {
                    value: note.value,
                    fee: chosen_fee,
                });
            }
            witness.fee = chosen_fee;
            witness.public_amount = public_amount;
            let mut tx = assemble_spend(
                self.indexer.pool().address,
                chain_id,
                &keys,
                &tuple,
                dummy_proof.clone(),
                &spend_struct(
                    &witness, root_slot, epoch, &nfs, &outs, recipient, authorizer,
                ),
                tail.as_ref(),
                authorizer.address(),
                max_priority_fee,
                max_fee,
            );
            tx.sign_secp256k1(0, authorizer)?;
            let cost = alloy_to_ruint(tx.max_cost());
            if chosen_fee >= cost {
                break;
            }
            chosen_fee = cost;
        }

        if chosen_fee >= note.value {
            return Err(ProviderError::FeeExceedsValue {
                value: note.value,
                fee: chosen_fee,
            });
        }
        witness.fee = chosen_fee;
        witness.public_amount = note.value - chosen_fee;

        let circuit_proof =
            prove(&witness.circuit_inputs()).map_err(|e| ProviderError::Circuit(e.to_string()))?;
        let proof_bytes = Bytes::copy_from_slice(&circuit_proof.to_frame_bytes());
        let nfs = witness.nullifiers();
        let outs = witness.output_commitments();
        let mut tx = assemble_spend(
            self.indexer.pool().address,
            chain_id,
            &keys,
            &tuple,
            proof_bytes,
            &spend_struct(
                &witness, root_slot, epoch, &nfs, &outs, recipient, authorizer,
            ),
            tail.as_ref(),
            authorizer.address(),
            max_priority_fee,
            max_fee,
        );
        tx.sign_secp256k1(0, authorizer)?;
        Ok(UnshieldResult {
            tx,
            public_amount: witness.public_amount,
            fee: witness.fee,
        })
    }

    /// Encode [`UnshieldHook.complete`] for a leftover call.
    #[must_use]
    pub fn hook_complete(
        hook: Address,
        factory: Address,
        create: &CreateAccount,
        calls: &[Call],
        signature: Bytes,
    ) -> WildCall {
        let batch: Vec<UnshieldHook::Call> = calls
            .iter()
            .map(|c| UnshieldHook::Call {
                target: c.target,
                value: c.value,
                data: c.data.clone(),
            })
            .collect();
        WildCall {
            target: hook,
            data: Bytes::from(
                UnshieldHook::completeCall {
                    factory,
                    owner: create.owner,
                    salt: create.salt,
                    calls: batch,
                    signature,
                }
                .abi_encode(),
            ),
        }
    }

    /// Owner ECDSA over `(chainId, account, nonce, calls)` for [`FrameAccount`].
    ///
    /// # Errors
    /// Returns if signing fails.
    pub fn sign_execute_batch(
        chain_id: u64,
        account: Address,
        nonce: u64,
        calls: &[Call],
        signer: &PrivateKeySigner,
    ) -> Result<Bytes, ProviderError> {
        let batch: Vec<AccountCall> = calls
            .iter()
            .map(|c| AccountCall {
                target: c.target,
                value: c.value,
                data: c.data.clone(),
            })
            .collect();
        sign_account_batch(chain_id, account, nonce, &batch, signer)
    }
}

fn validate_tail(tail: &TailCall) -> Result<(), ProviderError> {
    if tail.target.is_zero() {
        return Err(ProviderError::ZeroTailTarget);
    }
    if tail.data.len() > ACTION_FRAME_MAX_CALLDATA {
        return Err(ProviderError::TailTooLarge {
            got: tail.data.len(),
            cap: ACTION_FRAME_MAX_CALLDATA,
        });
    }
    if tail.execution_gas > ACTION_FRAME_MAX_GAS {
        return Err(ProviderError::TailGas {
            got: tail.execution_gas,
            cap: ACTION_FRAME_MAX_GAS,
        });
    }
    if tail.state_gas > ACTION_FRAME_MAX_STATE_GAS {
        return Err(ProviderError::TailGas {
            got: tail.state_gas,
            cap: ACTION_FRAME_MAX_STATE_GAS,
        });
    }
    Ok(())
}

fn spend_struct(
    witness: &SpendWitness,
    root_slot: u64,
    epoch: u64,
    nfs: &[U256; 2],
    outs: &[U256; 2],
    recipient: Address,
    authorizer: &PrivateKeySigner,
) -> SolSpend {
    SolSpend {
        root: u256_to_b256(witness.root),
        rootSlot: root_slot,
        epoch,
        domain: u256_to_b256(witness.domain),
        nf1: u256_to_b256(nfs[0]),
        nf2: u256_to_b256(nfs[1]),
        outCm1: u256_to_b256(outs[0]),
        outCm2: u256_to_b256(outs[1]),
        publicAmount: alloy_u256(witness.public_amount),
        fee: alloy_u256(witness.fee),
        recipient,
        authorizer: authorizer.address(),
    }
}

#[allow(clippy::too_many_arguments)]
fn assemble_spend(
    pool: Address,
    chain_id: u64,
    keys: &[AlloyU256; 2],
    tuple: &Bytes,
    proof: Bytes,
    spend: &SolSpend,
    tail: Option<&TailCall>,
    authorizer: Address,
    max_priority_fee: AlloyU256,
    max_fee: AlloyU256,
) -> FrameTx {
    let settle = Bytes::from(ShieldedPool::settleCall { s: spend.clone() }.abi_encode());
    let mut frames = vec![
        Frame {
            mode: FRAME_MODE_VERIFY,
            flags: 0,
            target: Some(RECENT_ROOT_ADDRESS),
            execution_gas: RECENT_ROOT_FRAME_GAS,
            state_gas: 0,
            value: AlloyU256::ZERO,
            data: tuple.clone(),
        },
        Frame {
            mode: FRAME_MODE_VERIFY,
            flags: APPROVE_EXECUTION_AND_PAYMENT,
            target: Some(pool),
            execution_gas: VERIFY_FRAME_GAS,
            state_gas: VERIFY_FRAME_STATE_GAS,
            value: AlloyU256::ZERO,
            data: proof,
        },
        Frame {
            mode: FRAME_MODE_SENDER,
            flags: 0,
            target: Some(pool),
            execution_gas: SETTLE_FRAME_GAS,
            state_gas: SETTLE_FRAME_STATE_GAS,
            value: AlloyU256::ZERO,
            data: settle,
        },
    ];
    if let Some(t) = tail {
        frames.push(Frame {
            mode: FRAME_MODE_DEFAULT,
            flags: 0,
            target: Some(t.target),
            execution_gas: t.execution_gas,
            state_gas: t.state_gas,
            value: AlloyU256::ZERO,
            data: t.data.clone(),
        });
    }
    FrameTx {
        chain_id,
        nonce_keys: keys.to_vec(),
        nonce_seq: 0,
        sender: pool,
        frames,
        signatures: vec![FrameSig::secp256k1(authorizer)],
        max_priority_fee,
        max_fee,
        max_blob_fee: AlloyU256::ZERO,
        blob_hashes: vec![],
    }
}

fn sign_account_batch(
    chain_id: u64,
    account: Address,
    nonce: u64,
    calls: &[AccountCall],
    signer: &PrivateKeySigner,
) -> Result<Bytes, ProviderError> {
    let inner = keccak256(
        (
            AlloyU256::from(chain_id),
            account,
            AlloyU256::from(nonce),
            calls,
        )
            .abi_encode(),
    );
    let mut wrapped = Vec::with_capacity(60);
    wrapped.extend_from_slice(b"\x19Ethereum Signed Message:\n32");
    wrapped.extend_from_slice(inner.as_slice());
    let digest = keccak256(wrapped);
    let sig = signer
        .sign_hash_sync(&digest)
        .map_err(|e| ProviderError::Signer(e.to_string()))?;
    let mut raw = Vec::with_capacity(65);
    raw.extend_from_slice(&sig.r().to_be_bytes::<32>());
    raw.extend_from_slice(&sig.s().to_be_bytes::<32>());
    let mut v = u8::from(sig.v());
    if v < 27 {
        v += 27;
    }
    raw.push(v);
    Ok(raw.into())
}

fn u256_to_b256(v: U256) -> B256 {
    B256::from_slice(&v.to_be_bytes::<32>())
}

fn alloy_u256(v: U256) -> AlloyU256 {
    AlloyU256::from_be_bytes(v.to_be_bytes::<32>())
}

fn alloy_to_ruint(v: AlloyU256) -> U256 {
    U256::from_be_bytes(v.to_be_bytes::<32>())
}
