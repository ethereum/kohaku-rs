//! Minimal RLP matching [`devnet/frametx.py`](../../../minimal-shielded-pool/devnet/frametx.py).

use alloy::primitives::{Address, B256, Bytes, U256};

pub fn rlp_bytes(b: &[u8]) -> Vec<u8> {
    if b.len() == 1 && b[0] < 0x80 {
        return b.to_vec();
    }
    encode_string(b)
}

fn encode_string(b: &[u8]) -> Vec<u8> {
    if b.len() < 56 {
        let mut out = Vec::with_capacity(1 + b.len());
        out.push(0x80 + u8::try_from(b.len()).expect("len < 56"));
        out.extend_from_slice(b);
        out
    } else {
        let lb = len_bytes(b.len());
        let mut out = Vec::with_capacity(1 + lb.len() + b.len());
        out.push(0xb7 + u8::try_from(lb.len()).expect("len bytes"));
        out.extend_from_slice(&lb);
        out.extend_from_slice(b);
        out
    }
}

pub fn rlp_list(items: &[Vec<u8>]) -> Vec<u8> {
    let body_len: usize = items.iter().map(Vec::len).sum();
    let mut body = Vec::with_capacity(body_len);
    for item in items {
        body.extend_from_slice(item);
    }
    if body.len() < 56 {
        let mut out = Vec::with_capacity(1 + body.len());
        out.push(0xc0 + u8::try_from(body.len()).expect("len < 56"));
        out.extend_from_slice(&body);
        out
    } else {
        let lb = len_bytes(body.len());
        let mut out = Vec::with_capacity(1 + lb.len() + body.len());
        out.push(0xf7 + u8::try_from(lb.len()).expect("len bytes"));
        out.extend_from_slice(&lb);
        out.extend_from_slice(&body);
        out
    }
}

pub fn rlp_int(x: impl Into<U256>) -> Vec<u8> {
    let x = x.into();
    if x.is_zero() {
        return rlp_bytes(&[]);
    }
    let be = x.to_be_bytes::<32>();
    let start = be.iter().position(|&b| b != 0).unwrap_or(31);
    rlp_bytes(&be[start..])
}

pub fn rlp_u64(x: u64) -> Vec<u8> {
    rlp_int(U256::from(x))
}

pub fn rlp_addr(addr: Address) -> Vec<u8> {
    rlp_bytes(addr.as_slice())
}

pub fn rlp_opt_addr(addr: Option<Address>) -> Vec<u8> {
    match addr {
        Some(a) => rlp_addr(a),
        None => rlp_bytes(&[]),
    }
}

pub fn rlp_hash(h: B256) -> Vec<u8> {
    rlp_bytes(h.as_slice())
}

pub fn rlp_data(data: &Bytes) -> Vec<u8> {
    rlp_bytes(data.as_ref())
}

fn len_bytes(len: usize) -> Vec<u8> {
    let bits = usize::BITS - len.leading_zeros();
    let nbytes = bits.div_ceil(8) as usize;
    len.to_be_bytes()[size_of::<usize>() - nbytes..].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rlp_int_zero_is_empty_string() {
        assert_eq!(rlp_int(U256::ZERO), vec![0x80]);
    }

    #[test]
    fn rlp_int_strips_leading_zeros() {
        assert_eq!(rlp_int(U256::from(0x5208u64)), vec![0x82, 0x52, 0x08]);
    }
}
