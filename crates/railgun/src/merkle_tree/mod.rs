pub mod verifier;

mod merkle_tree;
mod smart_wallet_verifier;
mod utxo_tree;

pub use merkle_tree::{
    MerkleProof, MerkleRoot, MerkleTree, MerkleTreeError, MerkleTreeState, NumberedMerkleTree,
    TOTAL_LEAVES, TREE_DEPTH, proof_indices,
};
pub use smart_wallet_verifier::SmartWalletUtxoVerifier;
pub use utxo_tree::{UtxoLeafHash, UtxoMerkleTree};
pub use verifier::MerkleTreeVerifier;
