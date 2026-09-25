use alloy::primitives::Address;
use kohaku_minimal_shield_circuit::{CircuitInputs, DEPTH};
use rand::CryptoRng;
use ruint::aliases::U256;
use thiserror::Error;

use crate::{
    crypto::{tagged, SINK_INNER_0, SINK_INNER_1, TAG_LEAF},
    note::Note,
};

/// One private merge: two notes in, one wallet note out, no public withdrawal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedMerge {
    pub inputs: [Note; 2],
    pub output: Note,
}

/// How to withdraw `public_amount` from a set of notes, merging first when
/// the 2-input circuit cannot cover the amount in one proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnshieldPlan {
    pub merges: Vec<PlannedMerge>,
    pub inputs: Vec<Note>,
    pub change: Option<Note>,
    pub public_amount: U256,
    pub fee: U256,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SelectError {
    #[error("amount must be positive")]
    ZeroAmount,
    #[error("notes cannot cover {amount} after fees")]
    Insufficient { amount: U256 },
}

/// Pick notes for an unshield of `amount`, reserving `fee_per_tx` on every
/// merge and on the final spend. Change and merge outputs are fresh notes.
///
/// # Errors
/// Returns [`SelectError::ZeroAmount`] when `amount` is zero, or
/// [`SelectError::Insufficient`] when the notes cannot pay `amount` and the fees.
pub fn plan_unshield(
    notes: &[Note],
    amount: U256,
    fee_per_tx: U256,
    rng: &mut impl CryptoRng,
) -> Result<UnshieldPlan, SelectError> {
    if amount.is_zero() {
        return Err(SelectError::ZeroAmount);
    }
    let mut pool: Vec<Note> = notes.to_vec();
    let mut merges = Vec::new();
    loop {
        if let Some((inputs, change)) = cover(&pool, amount, fee_per_tx, rng) {
            return Ok(UnshieldPlan {
                merges,
                inputs,
                change,
                public_amount: amount,
                fee: fee_per_tx,
            });
        }
        if pool.len() < 2 {
            return Err(SelectError::Insufficient { amount });
        }
        pool.sort_by(|a, b| a.value.cmp(&b.value));
        let left = pool.remove(0);
        let right = pool.remove(0);
        let Some(sum) = left.value.checked_add(right.value) else {
            return Err(SelectError::Insufficient { amount });
        };
        if sum <= fee_per_tx {
            return Err(SelectError::Insufficient { amount });
        }
        let output = Note::random(sum - fee_per_tx, left.chain_id, left.pool, rng);
        merges.push(PlannedMerge {
            inputs: [left, right],
            output: output.clone(),
        });
        pool.push(output);
    }
}

fn cover(
    notes: &[Note],
    amount: U256,
    fee: U256,
    rng: &mut impl CryptoRng,
) -> Option<(Vec<Note>, Option<Note>)> {
    let need = amount.checked_add(fee)?;
    if let Some(note) = notes
        .iter()
        .filter(|n| n.value >= need)
        .min_by(|a, b| a.value.cmp(&b.value))
    {
        return Some((vec![note.clone()], change_note(note, note.value - need, rng)));
    }
    let mut best: Option<(usize, usize)> = None;
    let mut best_sum = U256::MAX;
    for i in 0..notes.len() {
        for j in (i + 1)..notes.len() {
            let Some(sum) = notes[i].value.checked_add(notes[j].value) else {
                continue;
            };
            if sum >= need && sum < best_sum {
                best = Some((i, j));
                best_sum = sum;
            }
        }
    }
    let (i, j) = best?;
    let note = &notes[i];
    Some((
        vec![notes[i].clone(), notes[j].clone()],
        change_note(note, best_sum - need, rng),
    ))
}

fn change_note(template: &Note, value: U256, rng: &mut impl CryptoRng) -> Option<Note> {
    if value.is_zero() {
        None
    } else {
        Some(Note::random(value, template.chain_id, template.pool, rng))
    }
}

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
    /// Ten-value statement for hybrid compression / settle binding.
    #[must_use]
    pub fn statement(&self) -> [U256; 10] {
        let nfs = self.nullifiers();
        let outs = self.output_commitments();
        [
            nfs[0],
            nfs[1],
            outs[0],
            outs[1],
            self.root,
            self.domain,
            self.public_amount,
            self.fee,
            addr_to_u256(self.recipient),
            addr_to_u256(self.authorizer),
        ]
    }

    #[must_use]
    pub fn circuit_inputs(&self) -> CircuitInputs {
        let alpha = crate::crypto::compression_alpha(&self.statement());
        CircuitInputs {
            alpha,
            root: self.root,
            domain: self.domain,
            in_spend_key: [
                self.inputs[0]
                    .note
                    .as_ref()
                    .map_or(U256::ZERO, |n| n.spend_key),
                self.inputs[1]
                    .note
                    .as_ref()
                    .map_or(U256::ZERO, |n| n.spend_key),
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
            self.inputs[0].note.as_ref().map_or(U256::ZERO, |n| {
                n.nullifier(self.domain, index_from_bits(&self.inputs[0].bits))
            }),
            self.inputs[1].note.as_ref().map_or(U256::ZERO, |n| {
                n.nullifier(self.domain, index_from_bits(&self.inputs[1].bits))
            }),
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
fn index_from_bits(bits: &[U256; DEPTH]) -> U256 {
    bits.iter().enumerate().fold(U256::ZERO, |acc, (i, bit)| {
        if bit.is_zero() {
            acc
        } else {
            acc + (U256::from(1u64) << i)
        }
    })
}

#[must_use]
fn addr_to_u256(addr: Address) -> U256 {
    let mut w = [0u8; 32];
    w[12..].copy_from_slice(addr.as_slice());
    U256::from_be_bytes(w)
}

/// Output slots for a spend. A positive change note occupies slot 0; slot 1
/// stays the position-specific sink. Zero change uses both sinks.
#[must_use]
pub fn change_outputs(change: Option<&Note>) -> ([U256; 2], [U256; 2]) {
    let sinks = sink_outputs();
    match change {
        Some(note) if !note.value.is_zero() => {
            ([note.inner(), sinks[1].0], [note.value, sinks[1].1])
        }
        _ => ([sinks[0].0, sinks[1].0], [sinks[0].1, sinks[1].1]),
    }
}

#[must_use]
pub fn sink_outputs() -> [(U256, U256); 2] {
    [
        (U256::from(SINK_INNER_0), U256::ZERO),
        (U256::from(SINK_INNER_1), U256::ZERO),
    ]
}

#[cfg(test)]
mod tests {
    use super::{plan_unshield, SelectError};
    use crate::note::Note;
    use alloy::primitives::Address;
    use ruint::aliases::U256;

    fn note(value: u64) -> Note {
        let mut rng = rand::rng();
        Note::random(
            U256::from(value),
            8141,
            Address::repeat_byte(0x11),
            &mut rng,
        )
    }

    #[test]
    fn single_note_keeps_change() {
        let notes = vec![note(100), note(10)];
        let mut rng = rand::rng();
        let plan = plan_unshield(&notes, U256::from(40), U256::from(5), &mut rng).unwrap();
        assert!(plan.merges.is_empty());
        assert_eq!(plan.inputs.len(), 1);
        assert_eq!(plan.inputs[0].value, U256::from(100));
        let change = plan.change.unwrap();
        assert_eq!(change.value, U256::from(55));
    }

    #[test]
    fn pair_covers_when_no_single_note_does() {
        let notes = vec![note(30), note(30), note(5)];
        let mut rng = rand::rng();
        let plan = plan_unshield(&notes, U256::from(50), U256::from(5), &mut rng).unwrap();
        assert!(plan.merges.is_empty());
        assert_eq!(plan.inputs.len(), 2);
        assert_eq!(
            plan.inputs[0].value + plan.inputs[1].value,
            U256::from(60)
        );
        assert_eq!(plan.change.unwrap().value, U256::from(5));
    }

    #[test]
    fn merges_until_a_pair_covers() {
        let notes = vec![note(10), note(10), note(10)];
        let mut rng = rand::rng();
        let plan = plan_unshield(&notes, U256::from(25), U256::from(1), &mut rng).unwrap();
        assert_eq!(plan.merges.len(), 1);
        assert_eq!(plan.merges[0].output.value, U256::from(19));
        let input_sum: U256 = plan.inputs.iter().map(|n| n.value).sum();
        let change = plan.change.map(|n| n.value).unwrap_or(U256::ZERO);
        assert_eq!(input_sum, U256::from(25) + U256::from(1) + change);
    }

    #[test]
    fn insufficient_notes_error() {
        let notes = vec![note(10)];
        let mut rng = rand::rng();
        let err = plan_unshield(&notes, U256::from(20), U256::from(1), &mut rng).unwrap_err();
        assert_eq!(
            err,
            SelectError::Insufficient {
                amount: U256::from(20)
            }
        );
    }
}
