use ruint::aliases::U256;
use serde::{Deserialize, Serialize};

/// Groth16 proof in snarkjs / MSP proof-frame word order: `pA || pB || pC`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proof {
    pub a: [U256; 2],
    pub b: [[U256; 2]; 2],
    pub c: [U256; 2],
}

impl Proof {
    /// 256-byte VERIFY-frame calldata (`pA[0], pA[1], pB[0][0], pB[0][1], pB[1][0], pB[1][1], pC[0], pC[1]`).
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
            b: [
                [
                    proof.b.x.c0.into_bigint().into(),
                    proof.b.x.c1.into_bigint().into(),
                ],
                [
                    proof.b.y.c0.into_bigint().into(),
                    proof.b.y.c1.into_bigint().into(),
                ],
            ],
            c: [
                proof.c.x.into_bigint().into(),
                proof.c.y.into_bigint().into(),
            ],
        }
    }
}
