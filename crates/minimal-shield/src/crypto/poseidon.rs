use std::sync::OnceLock;

use ruint::aliases::U256;
use serde::Deserialize;

pub const P: U256 =
    ruint::uint!(21888242871839275222246405745257275088548364400416034343698204186575808495617_U256);
pub const TAG_PK: u64 = 1;
pub const TAG_LEAF: u64 = 2;
pub const TAG_NULL: u64 = 3;
pub const SINK_INNER_0: u64 = 1;
pub const SINK_INNER_1: u64 = 2;

#[derive(Deserialize)]
struct File {
    t3: Params,
    t4: Params,
}

#[derive(Deserialize)]
struct Params {
    rounds_f: u32,
    rounds_p: u32,
    #[serde(rename = "C")]
    c: Vec<String>,
    #[serde(rename = "M")]
    m: Vec<Vec<String>>,
}

struct Ready {
    rf: u32,
    rp: u32,
    c: Vec<U256>,
    m: Vec<Vec<U256>>,
}

fn parse_fe(s: &str) -> U256 {
    if let Some(hex) = s.strip_prefix("0x") {
        U256::from_str_radix(hex, 16).expect("poseidon const")
    } else {
        s.parse().expect("poseidon const")
    }
}

fn ready(p: &Params) -> Ready {
    Ready {
        rf: p.rounds_f,
        rp: p.rounds_p,
        c: p.c.iter().map(|s| parse_fe(s)).collect(),
        m: p.m
            .iter()
            .map(|row| row.iter().map(|s| parse_fe(s)).collect())
            .collect(),
    }
}

fn params(t: usize) -> &'static Ready {
    static T3: OnceLock<Ready> = OnceLock::new();
    static T4: OnceLock<Ready> = OnceLock::new();
    static FILE: OnceLock<File> = OnceLock::new();
    let file = FILE.get_or_init(|| {
        serde_json::from_str(include_str!("poseidon_bn254_constants.json")).expect("constants json")
    });
    match t {
        3 => T3.get_or_init(|| ready(&file.t3)),
        4 => T4.get_or_init(|| ready(&file.t4)),
        _ => panic!("unsupported poseidon t={t}"),
    }
}

fn add_mod(a: U256, b: U256) -> U256 {
    let (s, overflow) = a.overflowing_add(b);
    if overflow || s >= P {
        s.wrapping_sub(P)
    } else {
        s
    }
}

fn mul_mod(a: U256, b: U256) -> U256 {
    a.mul_mod(b, P)
}

fn pow5(x: U256) -> U256 {
    let x2 = mul_mod(x, x);
    let x4 = mul_mod(x2, x2);
    mul_mod(x4, x)
}

/// circomlib Poseidon: 2 or 3 field elements, output `state[0]`.
#[must_use]
pub fn poseidon(inputs: &[U256]) -> U256 {
    let t = inputs.len() + 1;
    let p = params(t);
    let mut state = vec![U256::ZERO];
    state.extend(inputs.iter().map(|x| *x % P));
    let rounds = p.rf + p.rp;
    for r in 0..rounds {
        for i in 0..t {
            state[i] = add_mod(state[i], p.c[r as usize * t + i]);
        }
        let full = r < p.rf / 2 || r >= p.rf / 2 + p.rp;
        if full {
            for s in &mut state {
                *s = pow5(*s);
            }
        } else {
            state[0] = pow5(state[0]);
        }
        let mut nxt = vec![U256::ZERO; t];
        for i in 0..t {
            let mut acc = U256::ZERO;
            for j in 0..t {
                acc = add_mod(acc, mul_mod(p.m[i][j], state[j]));
            }
            nxt[i] = acc;
        }
        state = nxt;
    }
    state[0]
}

#[must_use]
pub fn p2(a: U256, b: U256) -> U256 {
    poseidon(&[a, b])
}

#[must_use]
pub fn p3(a: U256, b: U256, c: U256) -> U256 {
    poseidon(&[a, b, c])
}

#[must_use]
pub fn tagged(tag: u64, a: U256, b: U256) -> U256 {
    p3(U256::from(tag), a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poseidon2_vectors() {
        let raw: serde_json::Value =
            serde_json::from_str(include_str!("poseidon_bn254_vectors.json")).unwrap();
        for v in raw["poseidon2"].as_array().unwrap().iter().take(6) {
            let a = parse_fe(v["in"][0].as_str().unwrap());
            let b = parse_fe(v["in"][1].as_str().unwrap());
            let out = parse_fe(v["out"].as_str().unwrap());
            assert_eq!(p2(a, b), out);
        }
    }
}
