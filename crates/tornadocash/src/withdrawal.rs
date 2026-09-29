use std::{array::from_fn, ops::Deref};

use alloy::{
    network::TransactionBuilder,
    primitives::{Address, Bytes},
    rpc::types::TransactionRequest,
    sol_types::SolCall,
};
use kohaku_tornadocash_circuit::{CircuitInputs, prove};
use rand::CryptoRng;
use ruint::aliases::U256;
use websnark_rs::proof::Proof;

use crate::{
    abis::tornado::Tornado::withdrawCall,
    merkle_tree::TcMerkleTree,
    note::Note,
    pool::Pool,
    relayer::{RelayerError, status::RelayerStatus},
};

#[derive(Debug, Clone)]
pub struct Withdrawal {
    pub pool: Pool,
    pub note: Note,
    pub recipient: Address,
    pub relayer: Option<Address>,
    pub fee: Option<U256>,
    pub refund: Option<U256>,
}

#[derive(Debug, Clone)]
pub struct ProvenWithdrawal {
    pub root: U256,
    pub proof: Proof,
    pub inner: Withdrawal,
}

#[derive(Debug, thiserror::Error)]
pub enum WithdrawalError {
    #[error("Merkle proof generation error: {0}")]
    MerkleProof(#[from] kohaku_merkle_tree::MerkleTreeError),
    #[error("Circuit error: {0}")]
    Circuit(#[from] kohaku_tornadocash_circuit::CircuitError),
    #[error("Proof generation error: {0}")]
    Proof(#[from] websnark_rs::proof::ProofError),
    #[error(transparent)]
    Relayer(#[from] RelayerError),
}

impl Withdrawal {
    pub fn new(pool: &Pool, note: Note, recipient: Address) -> Self {
        Self {
            pool: pool.clone(),
            note,
            recipient,
            relayer: None,
            fee: None,
            refund: None,
        }
    }

    pub fn with_relayer_address(mut self, relayer: Address) -> Self {
        self.relayer = Some(relayer);
        self
    }

    pub fn with_fee(mut self, fee: U256) -> Self {
        self.fee = Some(fee);
        self
    }

    pub fn with_refund(mut self, refund: U256) -> Self {
        self.refund = Some(refund);
        self
    }

    /// Pays `status`'s relayer to submit this withdrawal, at the fee it quotes for `gas_price`.
    ///
    /// # Errors
    /// Returns an error if the relayer does not support this withdrawal's pool.
    pub fn with_relayer(
        mut self,
        status: &RelayerStatus,
        gas_price: u128,
    ) -> Result<Self, WithdrawalError> {
        let fee = status.fee(&self.pool, gas_price, self.refund.unwrap_or_default())?;

        self.relayer = Some(status.reward_account);
        self.fee = Some(fee);
        Ok(self)
    }

    /// Generates the proof this withdrawal needs to be submitted.
    ///
    /// `tree` must already contain this withdrawal's note, and its root must still be within the
    /// pool's on-chain root history for the withdrawal to be accepted.
    ///
    /// # Errors
    /// Returns an error if the note is missing from `tree` or if proof generation fails.
    pub async fn prove(
        self,
        tree: &TcMerkleTree,
        rng: &mut impl CryptoRng,
    ) -> Result<ProvenWithdrawal, WithdrawalError> {
        let root = tree.root().await?;
        let merkle_proof = tree.leaf_proof(self.note.commitment()).await?;
        let path_elements = merkle_proof.siblings;
        let path_indices = from_fn(|i| U256::from(merkle_proof.path[i]));

        let circuit_inputs = CircuitInputs::new(
            root,
            self.note.nullifier_hash(),
            self.recipient.into_word().into(),
            self.relayer.unwrap_or_default().into_word().into(),
            self.fee.unwrap_or_default(),
            self.refund.unwrap_or_default(),
            self.note.nullifier.into(),
            self.note.secret.into(),
            path_elements,
            path_indices,
        );

        let proof = prove(&circuit_inputs, rng)?;
        Ok(ProvenWithdrawal {
            root,
            proof,
            inner: self,
        })
    }
}

impl ProvenWithdrawal {
    /// Returns the input data for this withdrawal transaction.
    #[must_use]
    pub fn input(&self) -> Vec<u8> {
        withdrawCall {
            _proof: self.proof_bytes(),
            _root: self.root.into(),
            _nullifierHash: self.note.nullifier_hash().into(),
            _recipient: self.recipient,
            _relayer: self.relayer.unwrap_or_default(),
            _fee: self.fee.unwrap_or_default(),
            _refund: self.refund.unwrap_or_default(),
        }
        .abi_encode()
    }

    /// Returns the target address for this withdrawal transaction.
    #[must_use]
    pub fn target(&self) -> Address {
        self.pool.address
    }

    /// Returns the proof in the format expected by the Solidity contract.
    #[must_use]
    pub fn proof_bytes(&self) -> Bytes {
        into_solidity_proof(&self.proof)
    }
}

impl Deref for ProvenWithdrawal {
    type Target = Withdrawal;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl From<ProvenWithdrawal> for TransactionRequest {
    fn from(proven_withdrawal: ProvenWithdrawal) -> Self {
        TransactionRequest::default()
            .with_to(proven_withdrawal.target())
            .input(proven_withdrawal.input().into())
    }
}

/// Convert a websnark proof into the format expected by the Solidity contract.
fn into_solidity_proof(proof: &Proof) -> Bytes {
    let proof_elements: [U256; 8] = [
        proof.a.x.into(),
        proof.a.y.into(),
        //? Order of b elements are reversed to match Solidity's expected format
        proof.b.x.c1.into(),
        proof.b.x.c0.into(),
        proof.b.y.c1.into(),
        proof.b.y.c0.into(),
        proof.c.x.into(),
        proof.c.y.into(),
    ];
    let mut proof_bytes = Vec::with_capacity(256);
    for elem in &proof_elements {
        proof_bytes.extend_from_slice(&elem.to_be_bytes::<32>());
    }

    proof_bytes.into()
}
