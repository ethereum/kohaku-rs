//! POI node wire types.

use kohaku_railgun::{circuit::proof::Proof, crypto::railgun_txid::Txid, merkle_tree::MerkleRoot};
use serde::{Deserialize, Serialize};

pub use kohaku_railgun::poi::types::{
    BlindedCommitment, BlindedCommitmentType, ListKey, PoiStatus, TxidVersion,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainParams {
    pub chain_type: String,
    #[serde(rename = "chainID")]
    pub chain_id: String,
    pub txid_version: TxidVersion,
}

/// Range of POI list leaves. `end_index` is exclusive. The node returns at most
/// 100 leaves and pads past the end of the list with the POI zero.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetPoiMerkletreeLeavesParams {
    #[serde(flatten)]
    pub chain: ChainParams,
    pub list_key: ListKey,
    pub start_index: u64,
    pub end_index: u64,
}

/// Asks whether these POI roots have been published. Carries no commitments.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidatePoiMerklerootsParams {
    #[serde(flatten)]
    pub chain: ChainParams,
    pub list_key: ListKey,
    pub poi_merkleroots: Vec<MerkleRoot>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ValidatedRailgunTxidStatus {
    #[serde(rename = "validatedTxidIndex")]
    pub index: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidateTxidMerklerootParams {
    #[serde(flatten)]
    pub chain: ChainParams,
    pub tree: u32,
    pub index: u32,
    pub merkleroot: MerkleRoot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitTransactProofParams {
    #[serde(flatten)]
    pub chain: ChainParams,
    pub list_key: ListKey,
    pub transact_proof_data: TransactProofData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactProofData {
    #[serde(rename = "snarkProof")]
    pub proof: Proof,
    pub poi_merkleroots: Vec<MerkleRoot>,
    /// Merkle root of the txid tree the inclusion proof was generated with
    pub txid_merkleroot: MerkleRoot,
    /// Index of the txid tree the inclusion proof was generated with
    ///
    /// NOT the leaf index of the txid, but the index for the merkleroot of the
    /// txid tree. If a single railgun transaction had multiple txids, this
    /// would be the same for all of them since they're all being proven against
    /// the same snapshot of the txid tree.
    pub txid_merkleroot_index: u64,
    pub blinded_commitments_out: Vec<BlindedCommitment>,
    pub railgun_txid_if_has_unshield: Txid,
}

impl ValidatedRailgunTxidStatus {
    pub fn tree(&self) -> u32 {
        (self.index >> 16) as u32
    }

    pub fn leaf_index(&self) -> u32 {
        self.index & 0xFFFF
    }
}
