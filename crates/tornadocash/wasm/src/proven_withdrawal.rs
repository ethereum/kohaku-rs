use kohaku_tornadocash::ProvenWithdrawal as CoreProvenWithdrawal;
use tsify::{Ts, Tsify};
use wasm_bindgen::prelude::*;

use crate::{call::Call, hex::Hex};

/// A Tornado Cash withdrawal with its generated zero-knowledge proof.
///
/// See [`kohaku_tornadocash::ProvenWithdrawal`] for more details.
#[wasm_bindgen]
pub struct ProvenWithdrawal {
    inner: CoreProvenWithdrawal,
}

impl From<CoreProvenWithdrawal> for ProvenWithdrawal {
    fn from(inner: CoreProvenWithdrawal) -> Self {
        Self { inner }
    }
}

#[wasm_bindgen]
impl ProvenWithdrawal {
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

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use alloy::primitives::{Address, U256};
    use kohaku_tornadocash::{
        Note as CoreNote, Payer as CorePayer, Pool as CorePool,
        merkle_tree::MerkleTree as CoreMerkleTree,
    };
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::*;
    use crate::{MerkleProof, Note, Payer, Pool, Withdrawal};

    #[wasm_bindgen_test]
    fn proves_in_wasm_with_system_entropy() {
        let pool = Pool {
            inner: CorePool::ETHEREUM_DAI_100,
        };
        let note = Note {
            inner: CoreNote::new([1; 31], [2; 31]),
        };
        let tree = CoreMerkleTree::from_leaves(&[note.inner.commitment()]).unwrap();
        let proof: MerkleProof = tree.proof(0).unwrap().into();
        let payer = Payer {
            inner: CorePayer {
                address: Address::with_last_byte(2),
                fee: U256::from(3),
                refund: U256::from(42),
            },
        };
        let withdrawal = Withdrawal::new(
            &pool,
            &note,
            &Hex::from(Address::with_last_byte(1)).into_ts().unwrap(),
        )
        .unwrap()
        .with_payer(&payer);
        withdrawal.prove(&proof).unwrap();
    }
}
