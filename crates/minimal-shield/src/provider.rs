use alloy::{
    primitives::{Address, B256, Bytes, U256 as AlloyU256, keccak256},
    signers::{SignerSync, local::PrivateKeySigner},
    sol_types::{SolCall, SolValue},
};
use kohaku_frametx_kit::{
    APPROVE_EXECUTION_AND_PAYMENT, CLAIM_FRAME_GAS, CLAIM_FRAME_STATE_GAS, EXECUTE_FRAME_GAS,
    EXECUTE_FRAME_STATE_GAS, FRAME_MODE_SENDER, FRAME_MODE_VERIFY, Frame, FrameSig, FrameTx,
    RECENT_ROOT_ADDRESS, RECENT_ROOT_FRAME_GAS, SETTLE_FRAME_GAS, SETTLE_FRAME_STATE_GAS,
    SHIELD_VERIFY_GAS, VERIFY_FRAME_GAS, VERIFY_FRAME_STATE_GAS, recent_root_tuple_bytes, source_id,
};
use kohaku_minimal_shield_circuit::prove;
use ruint::aliases::U256;
use thiserror::Error;

use crate::{
    abis::{
        FrameAccount::{self, Call as AccountCall},
        ShieldedPool::{self, Spend as SolSpend},
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
    #[error("non-empty executeBatch calls require the FrameAccount owner's signature")]
    CallsOnEoa,
    #[error("FrameAccount executeBatch requires the owner's signature")]
    MissingOwnerSig,
    #[error("owner signer does not match CREATE2 owner")]
    OwnerMismatch,
    #[error(transparent)]
    Frame(#[from] kohaku_frametx_kit::FrameTxError),
    #[error(transparent)]
    Indexer(#[from] crate::indexer::IndexerError),
}

#[derive(Clone)]
pub struct PoolProvider {
    pub indexer: Indexer,
}

#[derive(Debug, Clone)]
pub struct CreateAccount {
    pub factory: Address,
    pub owner: Address,
    pub salt: B256,
}

#[derive(Debug, Clone)]
pub struct Call {
    pub target: Address,
    pub value: AlloyU256,
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

    /// Always five frames. `create = None` skips CREATE2: an EOA if `owner_signer`
    /// is also `None`, or an **already-deployed** FrameAccount if `owner_signer`
    /// is `Some` (required for any FrameAccount, including empty `calls`;
    /// `execute_nonce` must be the live account nonce).
    ///
    /// # Errors
    /// Returns if the note is missing, calls target an EOA, or proving fails.
    pub async fn unshield(
        &self,
        note: &Note,
        dummy: &Note,
        recipient: Address,
        create: Option<CreateAccount>,
        calls: &[Call],
        owner_signer: Option<&PrivateKeySigner>,
        execute_nonce: u64,
        fee: U256,
        authorizer: &PrivateKeySigner,
        root_slot: u64,
        epoch: u64,
        chain_id: u64,
        max_priority_fee: AlloyU256,
        max_fee: AlloyU256,
    ) -> Result<FrameTx, ProviderError> {
        if owner_signer.is_none() && !calls.is_empty() {
            return Err(ProviderError::CallsOnEoa);
        }
        if create.is_some() && owner_signer.is_none() {
            return Err(ProviderError::MissingOwnerSig);
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

        let public_amount = note.value.saturating_sub(fee);
        let sinks = sink_outputs();
        let witness = SpendWitness {
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
            public_amount,
            fee,
            recipient,
            authorizer: authorizer.address(),
            root: proof.root,
            domain: note.domain(),
        };

        let circuit_proof =
            prove(&witness.circuit_inputs()).map_err(|e| ProviderError::Circuit(e.to_string()))?;
        let proof_bytes = Bytes::copy_from_slice(&circuit_proof.to_frame_bytes());

        let nfs = witness.nullifiers();
        let outs = witness.output_commitments();
        let mut keys = [alloy_u256(nfs[0]), alloy_u256(nfs[1])];
        keys.sort();

        let spend = SolSpend {
            root: u256_to_b256(witness.root),
            rootSlot: root_slot,
            epoch,
            domain: u256_to_b256(witness.domain),
            nf1: u256_to_b256(nfs[0]),
            nf2: u256_to_b256(nfs[1]),
            outCm1: u256_to_b256(outs[0]),
            outCm2: u256_to_b256(outs[1]),
            publicAmount: alloy_u256(public_amount),
            fee: alloy_u256(fee),
            recipient,
            authorizer: authorizer.address(),
        };
        let settle = Bytes::from(ShieldedPool::settleCall { s: spend }.abi_encode());

        let (factory, owner, salt) = match &create {
            Some(c) => (c.factory, c.owner, c.salt),
            None => (Address::ZERO, Address::ZERO, B256::ZERO),
        };
        let ensure = Bytes::from(
            ShieldedPool::ensureAndClaimCall {
                factory,
                owner,
                salt,
                who: recipient,
            }
            .abi_encode(),
        );
        let batch: Vec<AccountCall> = calls
            .iter()
            .map(|c| AccountCall {
                target: c.target,
                value: c.value,
                data: c.data.clone(),
            })
            .collect();
        let signature = if let Some(signer) = owner_signer {
            if let Some(c) = &create {
                if signer.address() != c.owner {
                    return Err(ProviderError::OwnerMismatch);
                }
            }
            sign_execute_batch(chain_id, recipient, execute_nonce, &batch, signer)?
        } else {
            Bytes::new()
        };
        let execute = Bytes::from(
            FrameAccount::executeBatchCall {
                calls: batch,
                signature,
            }
            .abi_encode(),
        );

        let src = source_id(self.indexer.pool().address, epoch);
        let tuple = recent_root_tuple_bytes(src, root_slot, u256_to_b256(witness.root));

        let mut tx = FrameTx {
            chain_id,
            nonce_keys: keys.to_vec(),
            nonce_seq: 0,
            sender: self.indexer.pool().address,
            frames: vec![
                Frame {
                    mode: FRAME_MODE_VERIFY,
                    flags: 0,
                    target: Some(RECENT_ROOT_ADDRESS),
                    execution_gas: RECENT_ROOT_FRAME_GAS,
                    state_gas: 0,
                    value: AlloyU256::ZERO,
                    data: tuple,
                },
                Frame {
                    mode: FRAME_MODE_VERIFY,
                    flags: APPROVE_EXECUTION_AND_PAYMENT,
                    target: Some(self.indexer.pool().address),
                    execution_gas: VERIFY_FRAME_GAS,
                    state_gas: VERIFY_FRAME_STATE_GAS,
                    value: AlloyU256::ZERO,
                    data: proof_bytes,
                },
                Frame {
                    mode: FRAME_MODE_SENDER,
                    flags: 0,
                    target: Some(self.indexer.pool().address),
                    execution_gas: SETTLE_FRAME_GAS,
                    state_gas: SETTLE_FRAME_STATE_GAS,
                    value: AlloyU256::ZERO,
                    data: settle,
                },
                Frame {
                    mode: FRAME_MODE_SENDER,
                    flags: 0,
                    target: Some(self.indexer.pool().address),
                    execution_gas: CLAIM_FRAME_GAS,
                    state_gas: CLAIM_FRAME_STATE_GAS,
                    value: AlloyU256::ZERO,
                    data: ensure,
                },
                Frame {
                    mode: FRAME_MODE_SENDER,
                    flags: 0,
                    target: Some(recipient),
                    execution_gas: EXECUTE_FRAME_GAS,
                    state_gas: EXECUTE_FRAME_STATE_GAS,
                    value: AlloyU256::ZERO,
                    data: execute,
                },
            ],
            signatures: vec![FrameSig::secp256k1(authorizer.address())],
            max_priority_fee,
            max_fee,
            max_blob_fee: AlloyU256::ZERO,
            blob_hashes: vec![],
        };
        tx.sign_secp256k1(0, authorizer)?;
        Ok(tx)
    }
}

fn sign_execute_batch(
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
    let signed = signer
        .sign_hash_sync(&digest)
        .map_err(|e| ProviderError::Signer(e.to_string()))?;
    let mut raw = Vec::with_capacity(65);
    raw.extend_from_slice(&signed.r().to_be_bytes::<32>());
    raw.extend_from_slice(&signed.s().to_be_bytes::<32>());
    let mut v = u8::from(signed.v());
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
