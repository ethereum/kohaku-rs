use alloy::{
    primitives::{keccak256, Address, Bytes, B256, U256 as AlloyU256},
    providers::Provider,
    signers::{local::PrivateKeySigner, SignerSync},
    sol_types::{SolCall, SolValue},
};
use kohaku_frametx_kit::{
    estimate_frame_account_tail_gas, recent_root_tuple_bytes, source_id, Frame, FrameSig, FrameTx,
    APPROVE_EXECUTION_AND_PAYMENT, CLAIM_FRAME_GAS, CLAIM_FRAME_STATE_GAS, CREATE2_MEASURE_EXEC,
    CREATE2_MEASURE_STATE, FRAME_MODE_DEFAULT, FRAME_MODE_SENDER, FRAME_MODE_VERIFY,
    RECENT_ROOT_ADDRESS, RECENT_ROOT_FRAME_GAS, SETTLE_FRAME_GAS, SETTLE_FRAME_STATE_GAS,
    SHIELD_VERIFY_GAS, VERIFY_FRAME_GAS, VERIFY_FRAME_STATE_GAS,
};
use kohaku_minimal_shield_circuit::{prove, NUM_PUBLIC_SIGNALS};
use ruint::aliases::U256;
use thiserror::Error;

use crate::{
    abis::{
        FrameAccount::{self, Call as AccountCall},
        FrameAccountFactory,
        Multicall3::{self, Call3},
        ShieldedPool::{self, Spend as SolSpend},
    },
    indexer::Indexer,
    note::Note,
    spend::{sink_outputs, SpendInput, SpendWitness},
};

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("circuit: {0}")]
    Circuit(String),
    #[error("signer: {0}")]
    Signer(String),
    #[error("note is not in the synced tree")]
    MissingNote,
    #[error("pool has no FrameAccount factory")]
    MissingFactory,
    #[error("note value {value} cannot cover fee {fee}")]
    FeeExceedsValue { value: U256, fee: U256 },
    #[error("account lookup: {0}")]
    Account(String),
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
    /// Circuit witness publics in verifier order (10 signals).
    pub circuit_publics: [U256; NUM_PUBLIC_SIGNALS],
    /// Precomputed FrameAccount, set by the account-tail spends.
    pub account: Option<Address>,
}

#[derive(Clone)]
pub struct PoolProvider {
    pub indexer: Indexer,
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

/// CREATE2 salt: `keccak256(abi.encodePacked("FRAMEACCT1", owner))`.
#[must_use]
pub fn frame_account_salt(owner: Address) -> B256 {
    let mut buf = [0u8; 30];
    buf[..10].copy_from_slice(b"FRAMEACCT1");
    buf[10..].copy_from_slice(owner.as_slice());
    keccak256(buf)
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
                    state_gas: SETTLE_FRAME_STATE_GAS,
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

    /// User-funded `publishEpochRoot`: same SelfVerify+SENDER shape as shield.
    /// A legacy type-2 send can sit in the Hegotá mempool forever if it fails to apply.
    #[must_use]
    pub fn publish_epoch(
        &self,
        sender: Address,
        nonce_seq: u64,
        chain_id: u64,
        max_priority_fee: AlloyU256,
        max_fee: AlloyU256,
        epoch: u64,
    ) -> FrameTx {
        let data = Bytes::from(ShieldedPool::publishEpochRootCall { epoch }.abi_encode());
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
                    state_gas: SETTLE_FRAME_STATE_GAS,
                    value: AlloyU256::ZERO,
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

    /// DEFAULT leftover: `claimWithdrawal(who)` targeting the pool.
    #[must_use]
    pub fn claim_tail(&self, who: Address) -> TailCall {
        TailCall {
            target: self.indexer.pool().address,
            data: Bytes::from(ShieldedPool::claimWithdrawalCall { who }.abi_encode()),
            execution_gas: CLAIM_FRAME_GAS,
            state_gas: CLAIM_FRAME_STATE_GAS,
        }
    }

    /// Withdraw the note into the owner's FrameAccount, then run `calls`.
    ///
    /// `note` and `owner` are raw secrets. CREATE2, the claim, and
    /// `executeBatch` are built here, in that order.
    ///
    /// # Errors
    /// Returns when the factory is missing, the account cannot be read, or
    /// [`Self::unshield`] fails.
    #[allow(clippy::too_many_arguments)]
    pub async fn unshield_with_tail<P: Provider + Sync>(
        &self,
        rpc: &P,
        note: &Note,
        dummy: &Note,
        owner: &PrivateKeySigner,
        calls: &[Call],
        multicall3: Address,
        create2_exec: u64,
        create2_state: u64,
        authorizer: &PrivateKeySigner,
        root_slot: u64,
        epoch: u64,
        chain_id: u64,
        max_priority_fee: AlloyU256,
        max_fee: AlloyU256,
    ) -> Result<UnshieldResult, ProviderError> {
        let (account, tail) = self
            .account_tail(
                rpc,
                owner,
                calls,
                multicall3,
                create2_exec,
                create2_state,
                chain_id,
                true,
            )
            .await?;
        let mut result = self
            .spend(
                note,
                dummy,
                account,
                Some(tail),
                None,
                authorizer,
                root_slot,
                epoch,
                chain_id,
                max_priority_fee,
                max_fee,
                false,
                Some(account),
            )
            .await?;
        result.account = Some(account);
        Ok(result)
    }

    /// Spend the whole note as the fee (`publicAmount = 0`) and run `calls`
    /// from the owner's FrameAccount. The account is deployed only if it has
    /// no code. There is no claim, because nothing is credited.
    ///
    /// # Errors
    /// Returns when the note cannot cover the padded fee, or the account
    /// lookup fails.
    #[allow(clippy::too_many_arguments)]
    pub async fn unshield_for_gas<P: Provider + Sync>(
        &self,
        rpc: &P,
        note: &Note,
        dummy: &Note,
        owner: &PrivateKeySigner,
        calls: &[Call],
        multicall3: Address,
        create2_exec: u64,
        create2_state: u64,
        authorizer: &PrivateKeySigner,
        root_slot: u64,
        epoch: u64,
        chain_id: u64,
        max_priority_fee: AlloyU256,
        max_fee: AlloyU256,
    ) -> Result<UnshieldResult, ProviderError> {
        let (account, tail) = self
            .account_tail(
                rpc,
                owner,
                calls,
                multicall3,
                create2_exec,
                create2_state,
                chain_id,
                false,
            )
            .await?;
        self.spend(
            note,
            dummy,
            Address::ZERO,
            Some(tail),
            None,
            authorizer,
            root_slot,
            epoch,
            chain_id,
            max_priority_fee,
            max_fee,
            true,
            Some(account),
        )
        .await
    }

    /// Three- or four-frame pool-as-sender spend. Frame 3 is a generic DEFAULT tail.
    ///
    /// `fee` of `None` uses [`FrameTx::max_cost`] (dispatcher `fee >= TXPARAM(0x06)`).
    ///
    /// # Errors
    /// Returns if the note is missing, the assembled tx exceeds EIP-7825
    /// execution or the 128 KiB mempool limit, the note cannot cover the fee,
    /// or proving fails.
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
        self.spend(
            note,
            dummy,
            recipient,
            tail,
            fee,
            authorizer,
            root_slot,
            epoch,
            chain_id,
            max_priority_fee,
            max_fee,
            false,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    async fn spend(
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
        gas_only: bool,
        account: Option<Address>,
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
            domain: note.domain(epoch),
        };

        let rust_nfs = witness.nullifiers();
        let rust_outs = witness.output_commitments();
        let mut keys = [alloy_u256(rust_nfs[0]), alloy_u256(rust_nfs[1])];
        keys.sort();
        let src = source_id(self.indexer.pool().address, epoch);
        let tuple = recent_root_tuple_bytes(src, root_slot, u256_to_b256(witness.root));
        let dummy_proof = Bytes::from(vec![1u8; 256]);

        if gas_only {
            witness.recipient = Address::ZERO;
        }
        let mut chosen_fee = fee.unwrap_or(U256::ZERO);
        for _ in 0..2 {
            let public_amount = if gas_only {
                U256::ZERO
            } else {
                note.value.saturating_sub(chosen_fee)
            };
            if !gas_only
                && public_amount.is_zero()
                && !note.value.is_zero()
                && chosen_fee >= note.value
            {
                return Err(ProviderError::FeeExceedsValue {
                    value: note.value,
                    fee: chosen_fee,
                });
            }
            witness.fee = if gas_only { note.value } else { chosen_fee };
            witness.public_amount = public_amount;
            let mut tx = assemble_spend(
                self.indexer.pool().address,
                chain_id,
                &keys,
                &tuple,
                dummy_proof.clone(),
                &spend_struct(
                    &witness, root_slot, epoch, &rust_nfs, &rust_outs, recipient, authorizer,
                ),
                tail.as_ref(),
                authorizer.address(),
                max_priority_fee,
                max_fee,
            );
            tx.sign_secp256k1(0, authorizer)?;
            tx.check_resource_limits()?;
            let cost = alloy_to_ruint(tx.max_cost());
            // Dispatcher VERIFY reverts if fee < TXPARAM(0x06). Pad in case our
            // max_cost underestimates the node's.
            let padded = cost + cost / U256::from(4);
            if chosen_fee >= padded {
                break;
            }
            chosen_fee = padded;
        }

        if gas_only {
            if note.value < chosen_fee {
                return Err(ProviderError::FeeExceedsValue {
                    value: note.value,
                    fee: chosen_fee,
                });
            }
            witness.recipient = Address::ZERO;
            witness.fee = note.value;
            witness.public_amount = U256::ZERO;
        } else if chosen_fee >= note.value {
            return Err(ProviderError::FeeExceedsValue {
                value: note.value,
                fee: chosen_fee,
            });
        } else {
            witness.fee = chosen_fee;
            witness.public_amount = note.value - chosen_fee;
        }

        let (circuit_proof, publics) =
            prove(&witness.circuit_inputs()).map_err(|e| ProviderError::Circuit(e.to_string()))?;
        let proof_bytes = Bytes::copy_from_slice(&circuit_proof.to_frame_bytes());
        // Settle must carry the witness publics the Groth16 proof was built
        // for, not a second rust recomputation of nf/out.
        let nfs = [publics[0], publics[1]];
        let outs = [publics[2], publics[3]];
        if nfs != rust_nfs || outs != rust_outs {
            eprintln!(
                "unshield: circuit nf/out differ from rust nf1_rust={:#x} nf1_circuit={:#x} nf2_rust={:#x} nf2_circuit={:#x} out1_rust={:#x} out1_circuit={:#x} out2_rust={:#x} out2_circuit={:#x}",
                rust_nfs[0],
                nfs[0],
                rust_nfs[1],
                nfs[1],
                rust_outs[0],
                outs[0],
                rust_outs[1],
                outs[1]
            );
        }
        witness.root = publics[4];
        witness.domain = publics[5];
        witness.public_amount = publics[6];
        witness.fee = publics[7];
        let mut keys = [alloy_u256(nfs[0]), alloy_u256(nfs[1])];
        keys.sort();
        // Frame 0 tuple must use the circuit root settle carries. Building it
        // before prove() would leave the indexer root in 8272 if they differ.
        let tuple = recent_root_tuple_bytes(src, root_slot, u256_to_b256(witness.root));
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
        tx.check_resource_limits()?;
        Ok(UnshieldResult {
            tx,
            public_amount: witness.public_amount,
            fee: witness.fee,
            circuit_publics: publics,
            account,
        })
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
        let batch = account_calls(calls);
        sign_account_batch(chain_id, account, nonce, &batch, signer)
    }

    async fn account_tail<P: Provider + Sync>(
        &self,
        rpc: &P,
        owner: &PrivateKeySigner,
        calls: &[Call],
        multicall3: Address,
        mut create2_exec: u64,
        mut create2_state: u64,
        chain_id: u64,
        include_claim: bool,
    ) -> Result<(Address, TailCall), ProviderError> {
        let factory = self.indexer.pool().factory;
        if factory.is_zero() {
            return Err(ProviderError::MissingFactory);
        }
        if create2_exec == 0 {
            create2_exec = CREATE2_MEASURE_EXEC;
        }
        if create2_state == 0 {
            create2_state = CREATE2_MEASURE_STATE;
        }
        let salt = frame_account_salt(owner.address());
        let account = FrameAccountFactory::new(factory, rpc)
            .getAddress(owner.address(), salt)
            .call()
            .await
            .map_err(|e| ProviderError::Account(e.to_string()))?;
        let code = rpc
            .get_code_at(account)
            .await
            .map_err(|e| ProviderError::Account(e.to_string()))?;
        let account_empty = code.is_empty();
        let nonce = if account_empty {
            0
        } else {
            let raw = FrameAccount::new(account, rpc)
                .nonce()
                .call()
                .await
                .map_err(|e| ProviderError::Account(e.to_string()))?;
            u64::try_from(raw)
                .map_err(|_| ProviderError::Account("nonce does not fit u64".into()))?
        };
        let mut empty_dests = 0u64;
        for call in calls {
            if target_is_empty(rpc, call.target).await? {
                empty_dests += 1;
            }
        }
        let (execution_gas, mut state_gas) = estimate_frame_account_tail_gas(
            account_empty,
            empty_dests > 0,
            include_claim,
            create2_exec,
            create2_state,
        );
        if empty_dests > 1 {
            state_gas = state_gas.saturating_add((empty_dests - 1) * CLAIM_FRAME_STATE_GAS);
        }
        let signature = Self::sign_execute_batch(chain_id, account, nonce, calls, owner)?;
        let tail = encode_account_tail(
            self.indexer.pool().address,
            multicall3,
            factory,
            owner.address(),
            salt,
            account,
            calls,
            signature,
            account_empty,
            include_claim,
            execution_gas,
            state_gas,
        );
        Ok((account, tail))
    }

    /// Digest [`FrameAccount::executeDigest`] must return for these calls.
    #[must_use]
    pub fn execute_batch_digest(
        chain_id: u64,
        account: Address,
        nonce: u64,
        calls: &[Call],
    ) -> B256 {
        account_batch_digest(chain_id, account, nonce, &account_calls(calls))
    }
}

fn encode_account_tail(
    pool: Address,
    multicall3: Address,
    factory: Address,
    owner: Address,
    salt: B256,
    account: Address,
    calls: &[Call],
    signature: Bytes,
    account_empty: bool,
    include_claim: bool,
    execution_gas: u64,
    state_gas: u64,
) -> TailCall {
    let exec = Bytes::from(
        FrameAccount::executeBatchCall {
            calls: account_calls(calls),
            signature,
        }
        .abi_encode(),
    );
    let include_create = include_claim || account_empty;
    if !include_create {
        return TailCall {
            target: account,
            data: exec,
            execution_gas,
            state_gas,
        };
    }
    let mut legs = vec![Call3 {
        target: factory,
        allowFailure: false,
        callData: Bytes::from(FrameAccountFactory::createAccountCall { owner, salt }.abi_encode()),
    }];
    if include_claim {
        legs.push(Call3 {
            target: pool,
            allowFailure: false,
            callData: Bytes::from(ShieldedPool::claimWithdrawalCall { who: account }.abi_encode()),
        });
    }
    legs.push(Call3 {
        target: account,
        allowFailure: false,
        callData: exec,
    });
    TailCall {
        target: multicall3,
        data: Bytes::from(Multicall3::aggregate3Call { calls: legs }.abi_encode()),
        execution_gas,
        state_gas,
    }
}

async fn target_is_empty(rpc: &impl Provider, target: Address) -> Result<bool, ProviderError> {
    let code = rpc
        .get_code_at(target)
        .await
        .map_err(|e| ProviderError::Account(e.to_string()))?;
    if !code.is_empty() {
        return Ok(false);
    }
    let balance = rpc
        .get_balance(target)
        .await
        .map_err(|e| ProviderError::Account(e.to_string()))?;
    if !balance.is_zero() {
        return Ok(false);
    }
    let nonce = rpc
        .get_transaction_count(target)
        .await
        .map_err(|e| ProviderError::Account(e.to_string()))?;
    Ok(nonce == 0)
}

fn validate_tail(tail: &TailCall) -> Result<(), ProviderError> {
    if tail.target.is_zero() {
        return Err(ProviderError::ZeroTailTarget);
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

fn account_calls(calls: &[Call]) -> Vec<AccountCall> {
    calls
        .iter()
        .map(|c| AccountCall {
            target: c.target,
            value: c.value,
            data: c.data.clone(),
        })
        .collect()
}

fn account_batch_digest(
    chain_id: u64,
    account: Address,
    nonce: u64,
    calls: &[AccountCall],
) -> B256 {
    // Solidity `abi.encode(...)` is params encoding. `abi_encode()` wraps the
    // tuple in one extra offset word, so `executeDigest` would not match.
    let inner = keccak256(
        (
            AlloyU256::from(chain_id),
            account,
            AlloyU256::from(nonce),
            calls,
        )
            .abi_encode_params(),
    );
    let mut wrapped = Vec::with_capacity(60);
    wrapped.extend_from_slice(b"\x19Ethereum Signed Message:\n32");
    wrapped.extend_from_slice(inner.as_slice());
    keccak256(wrapped)
}

fn sign_account_batch(
    chain_id: u64,
    account: Address,
    nonce: u64,
    calls: &[AccountCall],
    signer: &PrivateKeySigner,
) -> Result<Bytes, ProviderError> {
    let digest = account_batch_digest(chain_id, account, nonce, calls);
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

#[cfg(test)]
mod tests {
    use super::{frame_account_salt, Call, PoolProvider};
    use alloy::primitives::{address, b256, Address, Bytes, B256, U256};

    #[test]
    fn frame_account_salt_is_packed_tag_and_owner() {
        let owner = address!("0x1111111111111111111111111111111111111111");
        assert_eq!(
            frame_account_salt(owner),
            b256!("0xbc0ea624769fb1257ffa423f026945bdb07d598e7e61e8c8cb0031833b34220d")
        );
        assert_ne!(frame_account_salt(owner), frame_account_salt(Address::ZERO));
        let mut packed = [0u8; 30];
        packed[..10].copy_from_slice(b"FRAMEACCT1");
        packed[10..].copy_from_slice(owner.as_slice());
        assert_eq!(
            frame_account_salt(owner),
            B256::from(alloy::primitives::keccak256(packed))
        );
    }

    #[test]
    fn execute_batch_digest_matches_onchain_execute_digest() {
        let account = address!("0x055D4C56401fE20Ff5D0cB5F712586E70167e7d8");
        let calls = [Call {
            target: address!("0x4b5bad436cca8df3bd39a095b84991fac9a226f1"),
            value: U256::from(1_000_000_000_000_000u64),
            data: Bytes::new(),
        }];
        assert_eq!(
            PoolProvider::execute_batch_digest(8141, account, 0, &calls),
            b256!("0x7427ac0eb560f54e7d7aa102801e07cb704a975791387ad1cf1952ef59553b41")
        );
    }

    #[test]
    fn account_tail_order_is_create_claim_execute() {
        use super::encode_account_tail;
        use crate::abis::{FrameAccount, FrameAccountFactory, ShieldedPool};
        use alloy::sol_types::SolCall;

        let factory = address!("0x7BD5f77A0bbFB144d66337Dbb5c0755B34131adc");
        let pool = address!("0xac01c30f28b32dd31d3c2854012e673e74f6b100");
        let account = address!("0x055D4C56401fE20Ff5D0cB5F712586E70167e7d8");
        let owner = address!("0x88ae49c3529d0941f80dab882dcf6ec223dc36c7");
        let multicall = address!("0x6f273b85aa6384dd1e097f46eeae90cc026ce51b");
        let salt = frame_account_salt(owner);
        let calls = [Call {
            target: address!("0x4b5bad436cca8df3bd39a095b84991fac9a226f1"),
            value: U256::from(1_000_000_000_000_000u64),
            data: Bytes::new(),
        }];
        let tail = encode_account_tail(
            pool,
            multicall,
            factory,
            owner,
            salt,
            account,
            &calls,
            Bytes::from(vec![0u8; 65]),
            true,
            true,
            1,
            1,
        );
        let data = tail.data.as_ref();
        let create = &FrameAccountFactory::createAccountCall { owner, salt }.abi_encode()[..4];
        let claim = &ShieldedPool::claimWithdrawalCall { who: account }.abi_encode()[..4];
        let exec = &FrameAccount::executeBatchCall {
            calls: vec![],
            signature: Bytes::new(),
        }
        .abi_encode()[..4];
        let at = |sel: &[u8]| data.windows(4).position(|w| w == sel).unwrap();
        assert!(at(create) < at(claim) && at(claim) < at(exec));
        assert_eq!(tail.target, multicall);
    }
}
