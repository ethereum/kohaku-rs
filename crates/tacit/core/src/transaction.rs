//! A built `transact`, before and after proving.

use alloy::{
    network::TransactionBuilder,
    primitives::{Address, Bytes, U256 as AlloyU256},
    rpc::types::TransactionRequest,
    sol_types::SolCall,
};
use kohaku_tacit_circuit::{CircuitError, CircuitInputs, Proof, Prover};
use rand_core::CryptoRng;
use ruint::aliases::U256;
use serde::{Deserialize, Serialize};

use crate::{
    abi::TacitPool::transactCall,
    field::word,
    statement::{ExtData, Statement, i256},
};

/// A contract call: target, native value and calldata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub target: Address,
    /// Native currency to attach, in wei.
    pub value: AlloyU256,
    pub input: Bytes,
}

impl From<Call> for TransactionRequest {
    fn from(call: Call) -> Self {
        Self::default()
            .with_to(call.target)
            .with_value(call.value)
            .input(call.input.into())
    }
}

/// What kind of `transact` a [`Transaction`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Kind {
    /// ETH into the pool, sent by the user's own account.
    Shield,
    /// A private payment to another pool address.
    Transfer,
    /// ETH out of the pool to a public address.
    Unshield,
    /// Two notes merged into one.
    Consolidate,
}

/// A built `transact`, not yet proven.
#[derive(Debug, Clone)]
pub struct Transaction {
    pub(crate) kind: Kind,
    pub(crate) statement: Statement,
    pub(crate) ext: ExtData,
    /// What the operation moves: the shielded, paid, withdrawn or merged amount.
    pub(crate) amount: u128,
}

impl Transaction {
    #[must_use]
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// Everything the proof binds outside the circuit.
    #[must_use]
    pub fn ext_data(&self) -> &ExtData {
        &self.ext
    }

    #[must_use]
    pub fn amount(&self) -> u128 {
        self.amount
    }

    /// The relayer's fee, paid from the shielded funds.
    #[must_use]
    pub fn fee(&self) -> u128 {
        self.ext.fee
    }

    /// The circuit inputs.
    #[must_use]
    pub fn inputs(&self) -> &CircuitInputs {
        &self.statement.inputs
    }

    /// The nullifiers this transaction spends (zero for an empty slot).
    #[must_use]
    pub fn nullifiers(&self) -> [U256; 2] {
        self.statement.nullifiers.map(word)
    }

    /// Whether this transaction inserts leaves. One that does not (a withdrawal without change)
    /// proves against any root the pool has held; one that does must reach the pool's head.
    #[must_use]
    pub fn inserts(&self) -> bool {
        self.statement
            .out_leaves
            .iter()
            .any(|leaf| *leaf != crate::field::Fr::from(0u8))
    }

    /// Proves this transaction.
    ///
    /// # Errors
    /// Returns an error if proving fails or the proof does not verify.
    pub fn prove(
        self,
        prover: &Prover,
        rng: &mut impl CryptoRng,
    ) -> Result<ProvenTransaction, CircuitError> {
        let proof = prover.prove(&self.statement.inputs, rng)?;
        Ok(ProvenTransaction { tx: self, proof })
    }
}

/// A proven `transact`, ready to send from the user's account or hand to a relayer.
#[derive(Debug, Clone)]
pub struct ProvenTransaction {
    tx: Transaction,
    proof: Proof,
}

impl ProvenTransaction {
    #[must_use]
    pub fn transaction(&self) -> &Transaction {
        &self.tx
    }

    #[must_use]
    pub fn proof(&self) -> &Proof {
        &self.proof
    }

    /// The `transact` call. A shield attaches the shielded amount; every other operation attaches
    /// nothing.
    #[must_use]
    pub fn call(&self) -> Call {
        let ext = &self.tx.ext;
        let input = transactCall {
            pA: self.proof.pa(),
            pB: self.proof.pb(),
            pC: self.proof.pc(),
            publicInputs: self.tx.statement.inputs.public_signals(),
            recipient: ext.recipient,
            extAmount: i256(ext.ext_amount),
            relayer: ext.relayer,
            fee: AlloyU256::from(ext.fee),
            memo0: ext.memo0.clone(),
            memo1: ext.memo1.clone(),
        }
        .abi_encode();
        let value = ext.ext_amount.max(0).unsigned_abs();
        Call {
            target: ext.pool,
            value: AlloyU256::from(value),
            input: input.into(),
        }
    }

    /// The transaction in the form a relayer's `/relay` takes it.
    #[must_use]
    pub fn relay_request(&self) -> RelayTx {
        let s = |x: U256| x.to_string();
        let ext = &self.tx.ext;
        RelayTx {
            pa: self.proof.pa().map(s),
            pb: self.proof.pb().map(|pair| pair.map(s)),
            pc: self.proof.pc().map(s),
            public_inputs: self.tx.statement.inputs.public_signals().map(s).to_vec(),
            recipient: ext.recipient,
            ext_amount: ext.ext_amount.to_string(),
            relayer: ext.relayer,
            fee: ext.fee.to_string(),
            memo0: ext.memo0.clone(),
            memo1: ext.memo1.clone(),
        }
    }
}

/// The arguments of `transact` as a relayer takes them: integers as decimal strings, bytes as
/// hex.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayTx {
    #[serde(rename = "pA")]
    pub pa: [String; 2],
    #[serde(rename = "pB")]
    pub pb: [[String; 2]; 2],
    #[serde(rename = "pC")]
    pub pc: [String; 2],
    pub public_inputs: Vec<String>,
    pub recipient: Address,
    pub ext_amount: String,
    pub relayer: Address,
    pub fee: String,
    pub memo0: Bytes,
    pub memo1: Bytes,
}
