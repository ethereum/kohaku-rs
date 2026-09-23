use std::collections::HashMap;

use ruint::aliases::U256;

pub const DEPTH: usize = 20;

/// Circom public signal count: `nf1, nf2, out_cm1, out_cm2, root, domain,
/// public_amount, fee, recipient, authorizer`.
pub const NUM_PUBLIC_SIGNALS: usize = 10;

/// Private and public signals for `circuits/spend.circom`.
#[derive(Debug, Clone)]
pub struct CircuitInputs {
    pub root: U256,
    pub domain: U256,
    pub in_spend_key: [U256; 2],
    pub in_rho: [U256; 2],
    pub in_value: [U256; 2],
    pub in_siblings: [[U256; DEPTH]; 2],
    pub in_bits: [[U256; DEPTH]; 2],
    pub out_inner: [U256; 2],
    pub out_value: [U256; 2],
    pub public_amount: U256,
    pub fee: U256,
    pub recipient: U256,
    pub authorizer: U256,
}

impl CircuitInputs {
    #[must_use]
    pub fn to_circuit_signals(&self) -> HashMap<String, Vec<U256>> {
        let mut m = HashMap::new();
        m.insert("root".into(), vec![self.root]);
        m.insert("domain".into(), vec![self.domain]);
        m.insert("in_spend_key".into(), self.in_spend_key.to_vec());
        m.insert("in_rho".into(), self.in_rho.to_vec());
        m.insert("in_value".into(), self.in_value.to_vec());
        let mut sibs = Vec::with_capacity(DEPTH * 2);
        let mut bits = Vec::with_capacity(DEPTH * 2);
        for k in 0..2 {
            sibs.extend_from_slice(&self.in_siblings[k]);
            bits.extend_from_slice(&self.in_bits[k]);
        }
        m.insert("in_siblings".into(), sibs);
        m.insert("in_bits".into(), bits);
        m.insert("out_inner".into(), self.out_inner.to_vec());
        m.insert("out_value".into(), self.out_value.to_vec());
        m.insert("public_amount".into(), vec![self.public_amount]);
        m.insert("fee".into(), vec![self.fee]);
        m.insert("recipient".into(), vec![self.recipient]);
        m.insert("authorizer".into(), vec![self.authorizer]);
        m
    }
}
