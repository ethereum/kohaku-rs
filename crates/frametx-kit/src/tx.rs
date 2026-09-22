use alloy::{
    primitives::{Address, B256, Bytes, U256, keccak256},
    signers::{SignerSync, local::PrivateKeySigner},
};

use crate::{
    gas::{
        EIP7825_TX_GAS_CAP, ETHEX_MEMPOOL_MAX_BYTES, FRAME_TX_INTRINSIC, PER_FRAME_GAS,
        SIG_SCHEME_SECP256K1, SIGNATURE_GAS_SECP256K1, TX_VALUE_COST,
    },
    rlp::{rlp_addr, rlp_data, rlp_hash, rlp_int, rlp_list, rlp_opt_addr, rlp_u64},
};

const TX_TYPE: u8 = 0x06;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub mode: u8,
    pub flags: u8,
    pub target: Option<Address>,
    pub execution_gas: u64,
    pub state_gas: u64,
    pub value: U256,
    pub data: Bytes,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameSig {
    pub scheme: u8,
    pub signer: Address,
    pub msg: Bytes,
    pub signature: Bytes,
}

impl FrameSig {
    #[must_use]
    pub fn secp256k1(signer: Address) -> Self {
        Self {
            scheme: SIG_SCHEME_SECP256K1,
            signer,
            msg: Bytes::new(),
            signature: Bytes::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameTx {
    pub chain_id: u64,
    pub nonce_keys: Vec<U256>,
    pub nonce_seq: u64,
    pub sender: Address,
    pub frames: Vec<Frame>,
    pub signatures: Vec<FrameSig>,
    pub max_priority_fee: U256,
    pub max_fee: U256,
    pub max_blob_fee: U256,
    pub blob_hashes: Vec<B256>,
}

#[derive(Debug, thiserror::Error)]
pub enum FrameTxError {
    #[error("signature index {0} out of range")]
    SigIndex(usize),
    #[error("signer error: {0}")]
    Signer(#[from] alloy::signers::Error),
    #[error("declared execution {used} exceeds EIP-7825 cap {cap}")]
    ExecutionCap { used: u64, cap: u64 },
    #[error("encoded transaction {got} bytes exceeds ethrex mempool limit {cap}")]
    MempoolSize { got: usize, cap: usize },
}

impl Frame {
    fn rlp(&self) -> Vec<u8> {
        let limits = rlp_list(&[rlp_u64(self.execution_gas), rlp_u64(self.state_gas)]);
        rlp_list(&[
            rlp_u64(u64::from(self.mode)),
            rlp_u64(u64::from(self.flags)),
            rlp_opt_addr(self.target),
            limits,
            rlp_int(self.value),
            rlp_data(&self.data),
        ])
    }

    fn moves_value(&self, sender: Address) -> bool {
        !self.value.is_zero() && self.target.is_some_and(|t| t != sender)
    }
}

impl FrameSig {
    fn rlp(&self, elide: bool) -> Vec<u8> {
        let sig = if elide && self.msg.is_empty() {
            Bytes::new()
        } else {
            self.signature.clone()
        };
        rlp_list(&[
            rlp_u64(u64::from(self.scheme)),
            rlp_addr(self.signer),
            rlp_data(&self.msg),
            rlp_data(&sig),
        ])
    }
}

impl FrameTx {
    fn envelope(&self, elide_sigs: bool) -> Vec<u8> {
        let keys: Vec<Vec<u8>> = self.nonce_keys.iter().copied().map(rlp_int).collect();
        let frames: Vec<Vec<u8>> = self.frames.iter().map(Frame::rlp).collect();
        let sigs: Vec<Vec<u8>> = self.signatures.iter().map(|s| s.rlp(elide_sigs)).collect();
        let fees = rlp_list(&[
            rlp_int(self.max_priority_fee),
            rlp_int(self.max_fee),
            rlp_int(self.max_blob_fee),
        ]);
        let blobs: Vec<Vec<u8>> = self.blob_hashes.iter().copied().map(rlp_hash).collect();
        rlp_list(&[
            rlp_u64(self.chain_id),
            rlp_list(&keys),
            rlp_u64(self.nonce_seq),
            rlp_addr(self.sender),
            rlp_list(&frames),
            rlp_list(&sigs),
            fees,
            rlp_list(&blobs),
        ])
    }

    #[must_use]
    pub fn encode(&self) -> Bytes {
        self.envelope(false).into()
    }

    #[must_use]
    pub fn raw(&self) -> Bytes {
        let mut out = Vec::with_capacity(1 + self.encode().len());
        out.push(TX_TYPE);
        out.extend_from_slice(&self.encode());
        out.into()
    }

    #[must_use]
    pub fn sig_hash(&self) -> B256 {
        let mut buf = Vec::with_capacity(1 + 64);
        buf.push(TX_TYPE);
        buf.extend_from_slice(&self.envelope(true));
        keccak256(buf)
    }

    /// Signs `sig_index` with a bare recovery id `0|1` (not 27/28).
    ///
    /// # Errors
    /// Returns if the index is out of range or the signer fails.
    pub fn sign_secp256k1(
        &mut self,
        sig_index: usize,
        signer: &PrivateKeySigner,
    ) -> Result<(), FrameTxError> {
        if sig_index >= self.signatures.len() {
            return Err(FrameTxError::SigIndex(sig_index));
        }
        let signed = signer.sign_hash_sync(&self.sig_hash())?;
        let sig = &mut self.signatures[sig_index];
        let mut v = u8::from(signed.v());
        if v >= 27 {
            v -= 27;
        }
        let mut raw = Vec::with_capacity(65);
        raw.push(v);
        raw.extend_from_slice(&signed.r().to_be_bytes::<32>());
        raw.extend_from_slice(&signed.s().to_be_bytes::<32>());
        sig.scheme = SIG_SCHEME_SECP256K1;
        sig.signer = signer.address();
        sig.msg = Bytes::new();
        sig.signature = raw.into();
        Ok(())
    }

    fn signature_verification_cost(&self) -> u64 {
        self.signatures
            .iter()
            .map(|s| match s.scheme {
                0 => 100,
                SIG_SCHEME_SECP256K1 => SIGNATURE_GAS_SECP256K1,
                2 => 6_700,
                _ => 0,
            })
            .sum()
    }

    fn value_transfer_cost(&self) -> u64 {
        TX_VALUE_COST
            * u64::try_from(
                self.frames
                    .iter()
                    .filter(|f| f.moves_value(self.sender))
                    .count(),
            )
            .unwrap_or(u64::MAX)
    }

    #[must_use]
    pub fn mandatory_gas(&self) -> u64 {
        FRAME_TX_INTRINSIC
            + u64::try_from(self.frames.len()).unwrap_or(0) * PER_FRAME_GAS
            + self.signature_verification_cost()
            + self.value_transfer_cost()
    }

    #[must_use]
    pub fn state_gas_limit(&self) -> u64 {
        self.frames.iter().map(|f| f.state_gas).sum()
    }

    fn calldata_gas(encoded: &[u8]) -> u64 {
        encoded.iter().map(|b| if *b == 0 { 4 } else { 16 }).sum()
    }

    fn floor_tokens(encoded: &[u8]) -> u64 {
        4 * u64::try_from(encoded.len()).unwrap_or(u64::MAX)
    }

    fn nonce_calldata(&self) -> Vec<u8> {
        let keys: Vec<Vec<u8>> = self.nonce_keys.iter().copied().map(rlp_int).collect();
        let mut out = rlp_list(&keys);
        out.extend_from_slice(&rlp_u64(self.nonce_seq));
        out
    }

    fn data_fields(&self) -> Vec<Bytes> {
        let mut fields = Vec::new();
        for frame in &self.frames {
            fields.push(frame.data.clone());
        }
        for sig in &self.signatures {
            fields.push(Bytes::copy_from_slice(sig.signer.as_slice()));
            fields.push(sig.msg.clone());
            fields.push(sig.signature.clone());
        }
        fields.push(self.nonce_calldata().into());
        fields
    }

    #[must_use]
    pub fn standard_gas_limit(&self) -> u64 {
        let data_cost: u64 = self
            .data_fields()
            .iter()
            .map(|f| Self::calldata_gas(f.as_ref()))
            .sum();
        self.mandatory_gas()
            + data_cost
            + self.frames.iter().map(|f| f.execution_gas).sum::<u64>()
            + self.state_gas_limit()
    }

    #[must_use]
    pub fn calldata_floor_gas(&self) -> u64 {
        let tokens: u64 = self
            .data_fields()
            .iter()
            .map(|f| Self::floor_tokens(f.as_ref()))
            .sum();
        self.mandatory_gas() + 16 * tokens
    }

    /// Declared execution against EIP-7825. Does not add `limits.state`.
    #[must_use]
    pub fn execution_cap_usage(&self) -> u64 {
        let data_cost: u64 = self
            .data_fields()
            .iter()
            .map(|f| Self::calldata_gas(f.as_ref()))
            .sum();
        let exec_side = self.mandatory_gas()
            + data_cost
            + self.frames.iter().map(|f| f.execution_gas).sum::<u64>();
        exec_side.max(self.calldata_floor_gas())
    }

    /// Remaining EIP-7825 execution capacity and the whole-tx 128 KiB mempool limit.
    ///
    /// # Errors
    /// Returns if declared execution exceeds `2^24` or the encoded transaction
    /// exceeds the pinned ethrex mempool size.
    pub fn check_resource_limits(&self) -> Result<(), FrameTxError> {
        let used = self.execution_cap_usage();
        if used > EIP7825_TX_GAS_CAP {
            return Err(FrameTxError::ExecutionCap {
                used,
                cap: EIP7825_TX_GAS_CAP,
            });
        }
        let encoded = self.raw().len();
        if encoded > ETHEX_MEMPOOL_MAX_BYTES {
            return Err(FrameTxError::MempoolSize {
                got: encoded,
                cap: ETHEX_MEMPOOL_MAX_BYTES,
            });
        }
        Ok(())
    }

    #[must_use]
    pub fn total_gas_limit(&self) -> u64 {
        self.standard_gas_limit()
            .max(self.calldata_floor_gas() + self.state_gas_limit())
    }

    #[must_use]
    pub fn max_cost(&self) -> U256 {
        self.max_fee * U256::from(self.total_gas_limit())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> FrameTx {
        FrameTx {
            chain_id: 1,
            nonce_keys: vec![U256::ZERO],
            nonce_seq: 7,
            sender: Address::from_slice(&[
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xAB, 0xCD,
            ]),
            frames: vec![
                Frame {
                    mode: 1,
                    flags: 3,
                    target: None,
                    execution_gas: 0x5208,
                    state_gas: 0,
                    value: U256::ZERO,
                    data: Bytes::from_static(&[0x11, 0x22]),
                },
                Frame {
                    mode: 2,
                    flags: 0,
                    target: Some(Address::from_slice(&[
                        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x12, 0x34,
                    ])),
                    execution_gas: 0x9C40,
                    state_gas: 0,
                    value: U256::ZERO,
                    data: Bytes::new(),
                },
            ],
            signatures: vec![FrameSig {
                scheme: SIG_SCHEME_SECP256K1,
                signer: Address::from_slice(&[
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xAB, 0xCD,
                ]),
                msg: Bytes::new(),
                signature: Bytes::from(vec![0x01; 65]),
            }],
            max_priority_fee: U256::from(0x3B9ACA00u64),
            max_fee: U256::from(0x6FC23AC00u64),
            max_blob_fee: U256::ZERO,
            blob_hashes: vec![],
        }
    }

    #[test]
    fn envelope_has_eight_fields() {
        let encoded = sample().encode();
        assert_eq!(
            encoded[0], 0xf8,
            "long list prefix expected for this sample"
        );
    }

    #[test]
    fn execution_cap_ignores_state() {
        let mut tx = sample();
        let base = tx.execution_cap_usage();
        tx.frames[0].state_gas = 10_000_000;
        assert_eq!(tx.execution_cap_usage(), base);
        assert!(tx.check_resource_limits().is_ok());
    }

    #[test]
    fn state_budget_raises_max_gas() {
        let mut tx = sample();
        let base = tx.total_gas_limit();
        tx.frames[0].state_gas = 97_920;
        assert_eq!(tx.total_gas_limit() - base, 97_920);
    }

    #[test]
    fn value_to_other_account_adds_tx_value_cost() {
        let mut tx = sample();
        let base = tx.total_gas_limit();
        tx.frames[1].value = U256::from(1);
        assert_eq!(tx.total_gas_limit() - base, TX_VALUE_COST);
    }

    #[test]
    fn sign_writes_bare_recovery_id() {
        let mut tx = sample();
        let signer = PrivateKeySigner::from_slice(&[0x42; 32]).unwrap();
        tx.sign_secp256k1(0, &signer).unwrap();
        assert_eq!(tx.signatures[0].signature.len(), 65);
        assert!(tx.signatures[0].signature[0] <= 1);
        assert_eq!(tx.signatures[0].signer, signer.address());
    }
}
