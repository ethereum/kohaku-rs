use kohaku_tornadocash::ProvenWithdrawal as CoreProvenWithdrawal;
use tsify::{Ts, Tsify};
use wasm_bindgen::prelude::*;

use crate::{call::Call, hex::Hex};

/// A Tornado Cash withdrawal with its generated zero-knowledge proof.
///
/// See [`kohaku_tornadocash::ProvenWithdrawal`] for more details.
#[wasm_bindgen]
pub struct ProvenWithdrawal {
    pub(crate) inner: CoreProvenWithdrawal,
}

#[wasm_bindgen]
impl ProvenWithdrawal {
    /// Serialize this proven withdrawal, including its note secrets, as a JSON string.
    ///
    /// The format is unspecified; only `fromJSON` from the same package version reads it.
    ///
    /// # Errors
    /// Throws if the proven withdrawal cannot be serialized.
    #[wasm_bindgen(js_name = toJSON)]
    pub fn to_json(&self) -> Result<String, JsError> {
        Ok(serde_json::to_string(&self.inner)?)
    }

    /// Parse a proven withdrawal from the output of `toJSON`.
    ///
    /// # Errors
    /// Throws if `json` is not a serialized proven withdrawal.
    #[wasm_bindgen(js_name = fromJSON)]
    pub fn from_json(json: &str) -> Result<Self, JsError> {
        Ok(Self {
            inner: serde_json::from_str(json)?,
        })
    }

    /// Return the core's unsigned withdrawal call as plain JavaScript data.
    pub fn data(&self) -> Result<Ts<Call>, JsError> {
        Ok(Call::from(self.inner.call()).into_ts()?)
    }

    /// Return the Solidity-encoded proof as hex.
    #[wasm_bindgen(js_name = solidityProof)]
    pub fn solidity_proof(&self) -> Result<Ts<Hex>, JsError> {
        Ok(Hex::from(self.inner.solidity_proof().as_ref()).into_ts()?)
    }
}

impl From<CoreProvenWithdrawal> for ProvenWithdrawal {
    fn from(inner: CoreProvenWithdrawal) -> Self {
        Self { inner }
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use alloy::primitives::{Address, U256};
    use kohaku_tornadocash::{
        Note as CoreNote, Payer as CorePayer, Pool as CorePool, Withdrawal as CoreWithdrawal,
        merkle_tree::MerkleTree as CoreMerkleTree,
    };
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::*;
    use crate::{MerkleProof, Note, Payer, Pool, Withdrawal, circuit_artifacts};

    #[wasm_bindgen_test]
    fn json_round_trip_keeps_the_proven_withdrawal() {
        let note = CoreNote::new([1; 31], [2; 31]);
        let tree = CoreMerkleTree::from_leaves(&[note.commitment()]).unwrap();
        let proven = ProvenWithdrawal::from(
            CoreWithdrawal::new(
                &CorePool::ETHEREUM_DAI_100,
                note,
                Address::with_last_byte(1),
            )
            .with_payer(CorePayer {
                address: Address::with_last_byte(2),
                fee: U256::from(3),
                refund: U256::from(42),
            })
            .prove_dummy(&tree.proof(0).unwrap()),
        );

        let parsed = ProvenWithdrawal::from_json(&proven.to_json().unwrap()).unwrap();

        assert_eq!(parsed.inner, proven.inner);
    }

    #[wasm_bindgen_test]
    fn proves_in_wasm_with_system_entropy() {
        let pool = Pool::from(CorePool::ETHEREUM_DAI_100);
        let note = Note::from(CoreNote::new([1; 31], [2; 31]));
        let tree = CoreMerkleTree::from_leaves(&[note.inner.commitment()]).unwrap();
        let proof: MerkleProof = tree.proof(0).unwrap().into();
        let payer = Payer::from(CorePayer {
            address: Address::with_last_byte(2),
            fee: U256::from(3),
            refund: U256::from(42),
        });
        let mut withdrawal = Withdrawal::new(
            &pool,
            &note,
            &Hex::from(Address::with_last_byte(1)).into_ts().unwrap(),
        )
        .unwrap();
        withdrawal.with_payer(&payer);
        withdrawal
            .prove(&proof, &circuit_artifacts::test_utils::artifacts())
            .unwrap();
    }
}
