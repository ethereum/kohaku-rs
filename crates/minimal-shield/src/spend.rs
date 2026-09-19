use alloy::primitives::Address;
use kohaku_minimal_shield_circuit::{CircuitInputs, DEPTH};
use ruint::aliases::U256;

use crate::{
    crypto::{SINK_INNER_0, SINK_INNER_1, TAG_LEAF, tagged},
    note::Note,
};

#[derive(Debug, Clone)]
pub struct SpendInput {
    pub note: Option<Note>,
    pub siblings: [U256; DEPTH],
    pub bits: [U256; DEPTH],
}

impl SpendInput {
    #[must_use]
    pub fn dummy(note: Note) -> Self {
        Self {
            note: Some(Note {
                value: U256::ZERO,
                ..note
            }),
            siblings: [U256::ZERO; DEPTH],
            bits: [U256::ZERO; DEPTH],
        }
    }
}

#[derive(Debug, Clone)]
pub struct SpendWitness {
    pub inputs: [SpendInput; 2],
    pub out_inner: [U256; 2],
    pub out_value: [U256; 2],
    pub public_amount: U256,
    pub fee: U256,
    pub recipient: Address,
    pub authorizer: Address,
    pub root: U256,
    pub domain: U256,
}

impl SpendWitness {
    #[must_use]
    pub fn circuit_inputs(&self) -> CircuitInputs {
        CircuitInputs {
            root: self.root,
            domain: self.domain,
            in_spend_key: [
                self.inputs[0].note.as_ref().map_or(U256::ZERO, |n| n.spend_key),
                self.inputs[1].note.as_ref().map_or(U256::ZERO, |n| n.spend_key),
            ],
            in_rho: [
                self.inputs[0].note.as_ref().map_or(U256::ZERO, |n| n.rho),
                self.inputs[1].note.as_ref().map_or(U256::ZERO, |n| n.rho),
            ],
            in_value: [
                self.inputs[0].note.as_ref().map_or(U256::ZERO, |n| n.value),
                self.inputs[1].note.as_ref().map_or(U256::ZERO, |n| n.value),
            ],
            in_siblings: [self.inputs[0].siblings, self.inputs[1].siblings],
            in_bits: [self.inputs[0].bits, self.inputs[1].bits],
            out_inner: self.out_inner,
            out_value: self.out_value,
            public_amount: self.public_amount,
            fee: self.fee,
            recipient: addr_to_u256(self.recipient),
            authorizer: addr_to_u256(self.authorizer),
        }
    }

    #[must_use]
    pub fn nullifiers(&self) -> [U256; 2] {
        [
            self.inputs[0].note.as_ref().map_or(U256::ZERO, Note::nullifier),
            self.inputs[1].note.as_ref().map_or(U256::ZERO, Note::nullifier),
        ]
    }

    #[must_use]
    pub fn output_commitments(&self) -> [U256; 2] {
        [
            tagged(TAG_LEAF, self.out_inner[0], self.out_value[0]),
            tagged(TAG_LEAF, self.out_inner[1], self.out_value[1]),
        ]
    }
}

#[must_use]
fn addr_to_u256(addr: Address) -> U256 {
    let mut w = [0u8; 32];
    w[12..].copy_from_slice(addr.as_slice());
    U256::from_be_bytes(w)
}

#[must_use]
pub fn sink_outputs() -> [(U256, U256); 2] {
    [
        (U256::from(SINK_INNER_0), U256::ZERO),
        (U256::from(SINK_INNER_1), U256::ZERO),
    ]
}
