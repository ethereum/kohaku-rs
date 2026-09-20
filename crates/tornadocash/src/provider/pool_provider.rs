use std::array::from_fn;

use alloy::{
    network::TransactionBuilder,
    primitives::{Address, B256, Bytes},
    providers::{DynProvider, Provider},
    rpc::types::TransactionRequest,
    sol_types::SolCall,
};
use kohaku_tornadocash_circuit::{CircuitInputs, prove};
use rand::CryptoRng;
use ruint::aliases::U256;
use websnark_rs::proof::Proof;

use crate::{
    abis::tornado::Tornado,
    indexer::{Indexer, IndexerError},
    merkle_tree::TcMerkleTree,
    note::Note,
    pool::{Asset, Pool},
};

/// A provider for a single tornadocash pool.
///
/// The provider manages syncing and verifying the trie state, generating merkle proofs, and
/// creating deposit and withdrawal transactions.
#[derive(Clone)]
pub struct PoolProvider {
    indexer: Indexer,
    provider: DynProvider,
}

#[derive(Debug, thiserror::Error)]
pub enum PoolProviderError {
    #[error("Different pool: (chain_id, symbol, amount)({0}, {1}, {2}) != ({3}, {4}, {5})")]
    DifferentPool(u64, String, String, u64, String, String),
    #[error("Indexer error: {0}")]
    Indexer(#[from] IndexerError),
    #[error("Merkle proof generation error: {0}")]
    MerkleProof(#[from] kohaku_merkle_tree::MerkleTreeError),
    #[error("Circuit error: {0}")]
    Circuit(#[from] kohaku_tornadocash_circuit::CircuitError),
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
    pub fn new(indexer: Indexer, provider: DynProvider) -> Self {
        Self { indexer, provider }
    }

    /// Get the pool associated with this provider.
    #[must_use]
    pub fn pool(&self) -> Pool {
        self.indexer.pool()
    }

    /// Sync the provider to the latest block and verify the tree state.
    pub async fn sync(&self) -> Result<(), PoolProviderError> {
        Ok(self.indexer.sync().await?)
    }

    /// Create the withdrawal calldata for the given note to the recipient address.
    #[tracing::instrument(skip_all)]
    pub async fn prove_withdrawal(
        &self,
        note: &Note,
        recipient: Address,
        relayer: Option<Address>,
        fee: Option<U256>,
        refund: Option<U256>,
        mut rng: &mut impl CryptoRng,
    ) -> Result<Tornado::withdrawCall, PoolProviderError> {
        self.matches_pool(note)?;

        let merkle_tree = self.indexer.tree();
        let root = merkle_tree.root().await?;
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
            note.nullifier.into(),
            note.secret.into(),
            path_elements,
            path_indices,
        );

        let proof = prove(&circuit_inputs, &mut rng)?;
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

    /// Quote the amount of fee token from a given wei amount.
    pub async fn quote_wei_in_fee_token(
        &self,
        wei_amount: U256,
    ) -> Result<U256, PoolProviderError> {
        match self.pool().asset {
            Asset::Native { .. } => Ok(wei_amount),
            Asset::Erc20 { address, .. } => self.quote_wei_in_token(address, wei_amount).await,
        }
    }

    /// Returns if the given nullifier hash has been spent.
    pub async fn is_spent(&self, nullifier_hash: B256) -> Result<bool, PoolProviderError> {
        let call = Tornado::isSpentCall::new((nullifier_hash,)).abi_encode();

        let result = self
            .provider
            .call(
                TransactionRequest::default()
                    .with_to(self.pool().address)
                    .input(call.into()),
            )
            .await?;

        Ok(Tornado::isSpentCall::abi_decode_returns(&result)?)
    }

    /// Checks if the given note matches this provider's pool.
    #[must_use]
    fn matches_pool(&self, note: &Note) -> Result<(), PoolProviderError> {
        if note.chain_id != self.pool().chain_id
            || note.symbol != self.pool().symbol()
            || note.amount != self.pool().amount()
        {
            return Err(PoolProviderError::DifferentPool(
                note.chain_id,
                note.symbol.clone(),
                note.amount.clone(),
                self.pool().chain_id,
                self.pool().symbol(),
                self.pool().amount(),
            ));
        }

        Ok(())
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
