use std::array::from_fn;

use alloy::{
    network::TransactionBuilder,
    primitives::{Address, Bytes},
    providers::{DynProvider, Provider},
    rpc::types::TransactionRequest,
    sol_types::SolCall,
};
use kohaku_kv_store::Store;
use rand::CryptoRng;
use ruint::aliases::U256;
use websnark_rs::proof::Proof;

use crate::{
    abis::tornado::Tornado,
    circuit::{Circuit, input::CircuitInputs},
    indexer::{Indexer, IndexerError, syncer::Syncer, verifier::Verifier},
    merkle_tree::TcMerkleTree,
    provider::{
        call::Call,
        note::Note,
        pool::{Asset, Pool},
    },
};

/// A provider for a single tornadocash pool.
///
/// The provider manages syncing and verifying the trie state, generating merkle proofs, and
/// creating deposit and withdrawal transactions.
pub struct PoolProvider {
    indexer: Indexer,
    provider: DynProvider,
    circuit: Circuit,
}

#[derive(Debug, thiserror::Error)]
pub enum PoolProviderError {
    #[error("Invalid amount for pool: {0} != {1}")]
    InvalidAmount(String, String),
    #[error("Invalid symbol for pool: {0} != {1}")]
    InvalidSymbol(String, String),
    #[error("Indexer error: {0}")]
    Indexer(#[from] IndexerError),
    #[error("Merkle proof generation error: {0}")]
    MerkleProof(#[from] kohaku_merkle_tree::MerkleTreeError),
    #[error("Circuit error: {0}")]
    Circuit(#[from] crate::circuit::CircuitError),
    #[error("Proof generation error: {0}")]
    Proof(#[from] websnark_rs::proof::ProofError),
    #[error("Provider error: {0}")]
    Provider(#[from] alloy::transports::RpcError<alloy::transports::TransportErrorKind>),
    #[error("Sol error: {0}")]
    Sol(#[from] alloy::sol_types::Error),
}

impl PoolProvider {
    /// Creates a new pool provider for the given pool.
    #[must_use]
    pub fn new(
        pool: Pool,
        provider: DynProvider,
        store: Store,
        syncer: Syncer,
        verifier: Verifier,
        circuit: Circuit,
    ) -> Self {
        let indexer = Indexer::new(store, pool, syncer, verifier);
        Self {
            indexer,
            provider,
            circuit,
        }
    }

    /// Get the pool associated with this provider.
    #[must_use]
    pub fn pool(&self) -> &Pool {
        self.indexer.pool()
    }

    /// Sync the provider to the latest block and verify the tree state.
    ///
    /// # Errors
    /// Returns an error if the syncer or verifier fails.
    pub async fn sync(&mut self) -> Result<(), PoolProviderError> {
        self.indexer.sync().await?;
        self.verify().await
    }

    /// Sync the provider to a specific block.
    ///
    /// Will not verify the tree state after syncing because tornadocash
    /// only stores the merkle root for the past ~100 blocks.
    ///
    /// # Errors
    /// Returns an error if the syncer fails.
    pub async fn sync_to(&mut self, block: u64) -> Result<(), PoolProviderError> {
        Ok(self.indexer.sync_to(block).await?)
    }

    /// Verify the tree state of the provider.
    ///
    /// # Errors
    /// Returns an error if the verifier fails.
    pub async fn verify(&self) -> Result<(), PoolProviderError> {
        Ok(self.indexer.verify().await?)
    }

    /// Create a deposit transaction and note for this pool.
    #[tracing::instrument(skip_all)]
    pub fn deposit(&self, rng: &mut impl CryptoRng) -> (Call, Note) {
        let note = Note::random(
            &self.pool().symbol(),
            &self.pool().amount(),
            self.pool().chain_id,
            rng,
        );

        let calldata = Tornado::depositCall {
            _commitment: note.commitment().into(),
        }
        .abi_encode();
        let value = match self.pool().asset {
            Asset::Native { .. } => self.pool().amount_wei,
            Asset::Erc20 { .. } => 0,
        };

        let tx_data = Call::new(self.pool().address, calldata.into(), U256::from(value));
        (tx_data, note)
    }

    /// Create a withdrawal transaction for the given note to the recipient
    /// address.
    ///
    /// # Errors
    /// Returns an error if the withdrawal call cannot be created.
    pub async fn withdraw(
        &self,
        note: &Note,
        recipient: Address,
        relayer: Option<Address>,
        fee: Option<U256>,
        refund: Option<U256>,
        rng: &mut impl CryptoRng,
    ) -> Result<Call, PoolProviderError> {
        let call = self
            .withdraw_call(note, recipient, relayer, fee, refund, rng)
            .await?
            .abi_encode();

        Ok(Call::new(
            self.pool().address,
            call.into(),
            refund.unwrap_or_default(),
        ))
    }

    /// Create the withdrawal calldata for the given note to the recipient address.
    ///
    /// # Errors
    /// Returns an error if the note is invalid or the merkle proof cannot be generated.
    #[tracing::instrument(skip_all)]
    pub async fn withdraw_call(
        &self,
        note: &Note,
        recipient: Address,
        relayer: Option<Address>,
        fee: Option<U256>,
        refund: Option<U256>,
        mut rng: &mut impl CryptoRng,
    ) -> Result<Tornado::withdrawCall, PoolProviderError> {
        if note.amount != self.pool().amount() {
            return Err(PoolProviderError::InvalidAmount(
                note.amount.clone(),
                self.pool().amount(),
            ));
        }

        if note.symbol != self.pool().symbol() {
            return Err(PoolProviderError::InvalidSymbol(
                note.symbol.clone(),
                self.pool().symbol(),
            ));
        }

        let merkle_tree = self.indexer.tree();
        let root = merkle_tree.root().await;
        let nullifier_hash = note.nullifier_hash();
        let relayer = relayer.unwrap_or_default();
        let fee = fee.unwrap_or_default();
        let refund = refund.unwrap_or_default();

        let (path_elements, path_indices) = generate_merkle_proof(note, merkle_tree).await?;
        let circuit_inputs = CircuitInputs::new(
            root,
            nullifier_hash,
            recipient.into_word().into(),
            relayer.into_word().into(),
            fee,
            refund,
            U256::from_le_slice(&note.nullifier),
            U256::from_le_slice(&note.secret),
            path_elements,
            path_indices,
        );

        let proof = self.circuit.prove(&circuit_inputs, &mut rng)?;
        let proof = into_solidity_proof(&proof);
        let call = Tornado::withdrawCall {
            _proof: proof,
            _root: root.into(),
            _nullifierHash: nullifier_hash.into(),
            _recipient: recipient,
            _relayer: relayer,
            _fee: fee,
            _refund: refund,
        };

        Ok(call)
    }

    /// Quote the amount of fee token from a given wei amount. If the pool is native, this is a
    /// no-op.
    ///
    /// # Errors
    /// Returns an error if the quote cannot be queried.
    pub async fn quote_wei_in_fee_token(
        &self,
        wei_amount: U256,
    ) -> Result<U256, PoolProviderError> {
        match self.pool().asset {
            Asset::Native { .. } => Ok(wei_amount),
            Asset::Erc20 { address, .. } => self.quote_wei_in_token(address, wei_amount).await,
        }
    }

    async fn quote_wei_in_token(
        &self,
        token_address: Address,
        wei_amount: U256,
    ) -> Result<U256, PoolProviderError> {
        let call = Tornado::quoteWeiInTokenCall::new((token_address, wei_amount)).abi_encode();

        let result = self
            .provider
            .call(
                TransactionRequest::default()
                    .with_to(self.pool().address)
                    .input(call.into()),
            )
            .await?;

        let result = Tornado::quoteWeiInTokenCall::abi_decode_returns(&result)?;

        Ok(result)
    }
}

/// Generate the merkle proof for a given note and merkle tree.
///
/// Returns (siblings, path indices)
async fn generate_merkle_proof(
    note: &Note,
    merkle_tree: &TcMerkleTree,
) -> Result<([U256; 20], [U256; 20]), PoolProviderError> {
    let proof = merkle_tree.leaf_proof(note.commitment()).await?;

    let siblings = proof.siblings;
    let path_indices: [U256; 20] = from_fn(|i| U256::from(proof.path[i]));

    Ok((siblings, path_indices))
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
