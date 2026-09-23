use ruint::aliases::U256;
use serde::{Deserialize, Serialize};

/// Groth16 proof in snarkjs `export soliditycalldata` / MSP proof-frame order.
///
/// Each G2 `Fq2` is `[c1, c0]`, not arkworks `[c0, c1]`. Local ark verify uses
/// the ark point; the on-chain snarkjs verifier reads this swapped layout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proof {
    pub a: [U256; 2],
    pub b: [[U256; 2]; 2],
    pub c: [U256; 2],
}

impl Proof {
    /// 256-byte VERIFY-frame calldata (`pA || pB || pC`, eight 32-byte words).
    #[must_use]
    pub fn to_frame_bytes(&self) -> [u8; 256] {
        let words = [
            self.a[0],
            self.a[1],
            self.b[0][0],
            self.b[0][1],
            self.b[1][0],
            self.b[1][1],
            self.c[0],
            self.c[1],
        ];
        let mut out = [0u8; 256];
        for (i, w) in words.iter().enumerate() {
            out[i * 32..(i + 1) * 32].copy_from_slice(&w.to_be_bytes::<32>());
        }
        out
    }
}

impl From<ark_groth16::Proof<ark_bn254::Bn254>> for Proof {
    fn from(proof: ark_groth16::Proof<ark_bn254::Bn254>) -> Self {
        use ark_ff::PrimeField;
        Self {
            a: [
                proof.a.x.into_bigint().into(),
                proof.a.y.into_bigint().into(),
            ],
            // snarkjs `zkey export soliditycalldata` swaps each Fq2 limb.
            b: [
                [
                    proof.b.x.c1.into_bigint().into(),
                    proof.b.x.c0.into_bigint().into(),
                ],
                [
                    proof.b.y.c1.into_bigint().into(),
                    proof.b.y.c0.into_bigint().into(),
                ],
            ],
            c: [
                proof.c.x.into_bigint().into(),
                proof.c.y.into_bigint().into(),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_bytes_are_eight_words() {
        let p = Proof {
            a: [U256::from(1), U256::from(2)],
            b: [
                [U256::from(0x11), U256::from(0x10)],
                [U256::from(0x21), U256::from(0x20)],
            ],
            c: [U256::from(3), U256::from(4)],
        };
        let b = p.to_frame_bytes();
        assert_eq!(&b[64..96], &U256::from(0x11).to_be_bytes::<32>());
        assert_eq!(&b[96..128], &U256::from(0x10).to_be_bytes::<32>());
        assert_eq!(&b[128..160], &U256::from(0x21).to_be_bytes::<32>());
        assert_eq!(&b[160..192], &U256::from(0x20).to_be_bytes::<32>());
    }
}
