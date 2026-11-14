//! circomlib's Poseidon over BN254 (x⁵, 8 full rounds).
//!
//! The round constants and MDS matrix are generated from the Grain LFSR exactly as the Poseidon
//! reference does, the first time a width is used, so no constant tables are stored.

use std::sync::OnceLock;

use ark_ff::{AdditiveGroup, Field, PrimeField};

use crate::field::Fr;

struct Params {
    constants: Vec<Fr>,
    mds: Vec<Vec<Fr>>,
    partial: usize,
}

/// Partial rounds by state width `t` (inputs + 1), as circomlib sets them.
fn partial_rounds(t: usize) -> Option<usize> {
    match t {
        3 => Some(57),
        4 => Some(56),
        5 | 6 => Some(60),
        8 => Some(64),
        _ => None,
    }
}

const FULL: usize = 8;

struct Grain {
    bits: [u8; 80],
}

impl Grain {
    fn new(t: usize, partial: usize) -> Self {
        let mut bits = [1u8; 80];
        let mut at = 0;
        let mut put = |value: usize, n: usize| {
            for i in (0..n).rev() {
                bits[at] = u8::from((value >> i) & 1 == 1);
                at += 1;
            }
        };
        // field (prime), s-box (x^α), field size, width, full rounds, partial rounds; then 30 ones.
        put(1, 2);
        put(0, 4);
        put(254, 12);
        put(t, 12);
        put(FULL, 10);
        put(partial, 10);
        let mut grain = Self { bits };
        for _ in 0..160 {
            grain.step();
        }
        grain
    }

    fn step(&mut self) -> u8 {
        let b = &self.bits;
        let next = b[62] ^ b[51] ^ b[38] ^ b[23] ^ b[13] ^ b[0];
        self.bits.copy_within(1.., 0);
        self.bits[79] = next;
        next
    }

    fn bit(&mut self) -> u8 {
        loop {
            let keep = self.step();
            let bit = self.step();
            if keep == 1 {
                return bit;
            }
        }
    }

    /// 254 bits, most significant first.
    fn bytes(&mut self) -> [u8; 32] {
        let mut out = [0u8; 32];
        for i in 0..254 {
            let pos = i + 2;
            out[pos / 8] |= self.bit() << (7 - pos % 8);
        }
        out
    }

    /// A field element by rejection sampling.
    fn element(&mut self) -> Fr {
        loop {
            let bytes = self.bytes();
            let x = Fr::from_be_bytes_mod_order(&bytes);
            if x.into_bigint().to_bytes_be_32() == bytes {
                return x;
            }
        }
    }

    /// A field element by reduction.
    fn reduced(&mut self) -> Fr {
        Fr::from_be_bytes_mod_order(&self.bytes())
    }
}

trait Be32 {
    fn to_bytes_be_32(&self) -> [u8; 32];
}

impl Be32 for ark_ff::BigInt<4> {
    fn to_bytes_be_32(&self) -> [u8; 32] {
        let mut out = [0u8; 32];
        for (i, limb) in self.0.iter().enumerate() {
            out[32 - 8 * (i + 1)..32 - 8 * i].copy_from_slice(&limb.to_be_bytes());
        }
        out
    }
}

fn params(t: usize) -> Params {
    let partial = partial_rounds(t).expect("supported width");
    let mut grain = Grain::new(t, partial);
    let constants = (0..(FULL + partial) * t).map(|_| grain.element()).collect();
    let mds = loop {
        let xs: Vec<Fr> = (0..2 * t).map(|_| grain.reduced()).collect();
        let distinct = xs.iter().enumerate().all(|(i, x)| !xs[..i].contains(x));
        if !distinct {
            continue;
        }
        let rows: Vec<Vec<Fr>> = (0..t)
            .map(|i| (0..t).map(|j| xs[i] + xs[t + j]).collect())
            .collect();
        if rows.iter().flatten().all(|v| *v != Fr::ZERO) {
            break rows
                .into_iter()
                .map(|row| {
                    row.into_iter()
                        .map(|v| v.inverse().expect("non-zero"))
                        .collect()
                })
                .collect();
        }
    };
    Params {
        constants,
        mds,
        partial,
    }
}

fn cached(t: usize) -> &'static Params {
    static TABLES: [OnceLock<Params>; 9] = [const { OnceLock::new() }; 9];
    TABLES[t].get_or_init(|| params(t))
}

fn pow5(x: Fr) -> Fr {
    let x2 = x.square();
    x2.square() * x
}

/// circomlib's `Poseidon(n)` of `inputs`, for the widths the pool uses (2, 3, 4, 5 and 7 inputs).
///
/// # Panics
/// Panics for any other number of inputs.
pub(crate) fn poseidon(inputs: &[Fr]) -> Fr {
    let t = inputs.len() + 1;
    assert!(
        partial_rounds(t).is_some(),
        "no Poseidon parameters for {} inputs",
        inputs.len()
    );
    let params = cached(t);
    let rounds = FULL + params.partial;
    let mut state: Vec<Fr> = std::iter::once(Fr::ZERO)
        .chain(inputs.iter().copied())
        .collect();
    let mut next = vec![Fr::ZERO; t];
    let mut k = 0;
    for round in 0..rounds {
        for s in &mut state {
            *s += params.constants[k];
            k += 1;
        }
        if round < FULL / 2 || round >= rounds - FULL / 2 {
            for s in &mut state {
                *s = pow5(*s);
            }
        } else {
            state[0] = pow5(state[0]);
        }
        for (i, row) in params.mds.iter().enumerate() {
            next[i] = row.iter().zip(&state).map(|(m, s)| *m * s).sum();
        }
        std::mem::swap(&mut state, &mut next);
    }
    state[0]
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn f(s: &str) -> Fr {
        Fr::from_str(s).unwrap()
    }

    #[test]
    fn matches_circomlib() {
        // circomlibjs poseidon([1, 2]) and poseidon([1, 2, 3, 4]).
        assert_eq!(
            poseidon(&[f("1"), f("2")]),
            f("7853200120776062878684798364095072458815029376092732009249414926327459813530")
        );
        assert_eq!(
            poseidon(&[f("1"), f("2"), f("3"), f("4")]),
            f("18821383157269793795438455681495246036402687001665670618754263018637548127333")
        );
    }

    #[test]
    fn empty_pair_is_the_tree_constant() {
        assert_eq!(
            poseidon(&[Fr::ZERO, Fr::ZERO]),
            f("14744269619966411208579211824598458697587494354926760081771325075741142829156")
        );
    }
}
