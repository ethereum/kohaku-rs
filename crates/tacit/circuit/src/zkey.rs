//! Reader for snarkjs Groth16 proving keys (`.zkey`).
//!
//! Field elements are stored little-endian in Montgomery form (`R = 2^256`), which is also
//! arkworks' internal representation, so they are read without conversion. Coefficients carry one
//! more factor of `R` than points do. A point whose coordinates are all zero is the point at
//! infinity.

use ark_bn254::{Fq, Fq2, Fr, G1Affine, G2Affine};
use ark_ff::{AdditiveGroup, BigInt, PrimeField};

use crate::CircuitError;

/// One non-zero entry of the A or B matrix of the constraint system.
#[derive(Clone, Copy)]
pub(crate) struct Coefficient {
    /// `false` for A, `true` for B.
    pub b: bool,
    pub constraint: u32,
    pub signal: u32,
    pub value: Fr,
}

/// A Groth16 proving key over BN254.
pub(crate) struct ProvingKey {
    pub n_vars: usize,
    pub n_public: usize,
    pub domain_size: usize,
    pub alpha1: G1Affine,
    pub beta1: G1Affine,
    pub beta2: G2Affine,
    pub gamma2: G2Affine,
    pub delta1: G1Affine,
    pub delta2: G2Affine,
    pub ic: Vec<G1Affine>,
    pub coefficients: Vec<Coefficient>,
    pub a: Vec<G1Affine>,
    pub b1: Vec<G1Affine>,
    pub b2: Vec<G2Affine>,
    pub c: Vec<G1Affine>,
    pub h: Vec<G1Affine>,
}

const MAGIC: &[u8; 4] = b"zkey";
const GROTH16: u32 = 1;

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], CircuitError> {
        let end = self.at.checked_add(len).ok_or(zkey("length overflow"))?;
        let out = self.bytes.get(self.at..end).ok_or(zkey("truncated"))?;
        self.at = end;
        Ok(out)
    }

    fn u32(&mut self) -> Result<u32, CircuitError> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("4 bytes"),
        ))
    }

    fn u64(&mut self) -> Result<u64, CircuitError> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("8 bytes"),
        ))
    }

    fn limbs(&mut self) -> Result<BigInt<4>, CircuitError> {
        let bytes = self.take(32)?;
        Ok(BigInt(std::array::from_fn(|i| {
            u64::from_le_bytes(bytes[8 * i..8 * i + 8].try_into().expect("8 bytes"))
        })))
    }

    /// An element stored as its Montgomery form.
    fn fq(&mut self) -> Result<Fq, CircuitError> {
        let limbs = self.limbs()?;
        if limbs >= Fq::MODULUS {
            return Err(zkey("base field element out of range"));
        }
        Ok(Fq::new_unchecked(limbs))
    }

    /// A coefficient: its Montgomery form, multiplied by `R` once more.
    fn coefficient(&mut self) -> Result<Fr, CircuitError> {
        let limbs = self.limbs()?;
        if limbs >= Fr::MODULUS {
            return Err(zkey("scalar out of range"));
        }
        Ok(Fr::new_unchecked(Fr::new_unchecked(limbs).into_bigint()))
    }

    fn g1(&mut self) -> Result<G1Affine, CircuitError> {
        let (x, y) = (self.fq()?, self.fq()?);
        if x == Fq::ZERO && y == Fq::ZERO {
            return Ok(G1Affine::identity());
        }
        let point = G1Affine::new_unchecked(x, y);
        if !point.is_on_curve() {
            return Err(zkey("G1 point not on the curve"));
        }
        Ok(point)
    }

    fn g2(&mut self) -> Result<G2Affine, CircuitError> {
        let x = Fq2::new(self.fq()?, self.fq()?);
        let y = Fq2::new(self.fq()?, self.fq()?);
        if x == Fq2::ZERO && y == Fq2::ZERO {
            return Ok(G2Affine::identity());
        }
        let point = G2Affine::new_unchecked(x, y);
        if !point.is_on_curve() {
            return Err(zkey("G2 point not on the curve"));
        }
        Ok(point)
    }

    fn g1s(&mut self, count: usize) -> Result<Vec<G1Affine>, CircuitError> {
        (0..count).map(|_| self.g1()).collect()
    }

    fn g2s(&mut self, count: usize) -> Result<Vec<G2Affine>, CircuitError> {
        (0..count).map(|_| self.g2()).collect()
    }

    fn done(&self) -> Result<(), CircuitError> {
        if self.at == self.bytes.len() {
            Ok(())
        } else {
            Err(zkey("section has trailing bytes"))
        }
    }
}

fn zkey(what: &'static str) -> CircuitError {
    CircuitError::ProvingKey(what)
}

impl ProvingKey {
    /// Parses a snarkjs Groth16 proving key over BN254.
    pub(crate) fn read(bytes: &[u8]) -> Result<Self, CircuitError> {
        let mut file = Reader::new(bytes);
        if file.take(4)? != MAGIC {
            return Err(zkey("not a proving key"));
        }
        let _version = file.u32()?;
        let count = file.u32()?;

        let mut sections: [Option<&[u8]>; 11] = [None; 11];
        for _ in 0..count {
            let id = file.u32()? as usize;
            let len = usize::try_from(file.u64()?).map_err(|_| zkey("section too large"))?;
            let body = file.take(len)?;
            if let Some(slot) = sections.get_mut(id)
                && slot.replace(body).is_some()
            {
                return Err(zkey("repeated section"));
            }
        }
        let section = |id: usize| sections[id].map(Reader::new).ok_or(zkey("missing section"));

        let mut protocol = section(1)?;
        if protocol.u32()? != GROTH16 {
            return Err(zkey("not a Groth16 proving key"));
        }

        let mut header = section(2)?;
        let n8q = header.u32()?;
        let base_modulus = header.limbs()?;
        let n8r = header.u32()?;
        let scalar_modulus = header.limbs()?;
        if n8q != 32 || n8r != 32 || base_modulus != Fq::MODULUS || scalar_modulus != Fr::MODULUS {
            return Err(zkey("proving key is not over BN254"));
        }
        let n_vars = header.u32()? as usize;
        let n_public = header.u32()? as usize;
        let domain_size = header.u32()? as usize;
        if !domain_size.is_power_of_two() || n_public + 1 > n_vars {
            return Err(zkey("inconsistent header"));
        }
        let alpha1 = header.g1()?;
        let beta1 = header.g1()?;
        let beta2 = header.g2()?;
        let gamma2 = header.g2()?;
        let delta1 = header.g1()?;
        let delta2 = header.g2()?;
        header.done()?;

        let mut ic = section(3)?;
        let ic_points = ic.g1s(n_public + 1)?;
        ic.done()?;

        let mut coeffs = section(4)?;
        let n_coefficients = coeffs.u32()? as usize;
        let mut coefficients = Vec::with_capacity(n_coefficients);
        for _ in 0..n_coefficients {
            let matrix = coeffs.u32()?;
            let constraint = coeffs.u32()?;
            let signal = coeffs.u32()?;
            let value = coeffs.coefficient()?;
            if matrix > 1 || constraint as usize >= domain_size || signal as usize >= n_vars {
                return Err(zkey("coefficient out of range"));
            }
            coefficients.push(Coefficient {
                b: matrix == 1,
                constraint,
                signal,
                value,
            });
        }
        coeffs.done()?;

        let points = |id: usize, count: usize| -> Result<Vec<G1Affine>, CircuitError> {
            let mut reader = section(id)?;
            let out = reader.g1s(count)?;
            reader.done()?;
            Ok(out)
        };
        let a = points(5, n_vars)?;
        let b1 = points(6, n_vars)?;
        let mut b2_section = section(7)?;
        let b2 = b2_section.g2s(n_vars)?;
        b2_section.done()?;
        let c = points(8, n_vars - n_public - 1)?;
        let h = points(9, domain_size)?;

        Ok(Self {
            n_vars,
            n_public,
            domain_size,
            alpha1,
            beta1,
            beta2,
            gamma2,
            delta1,
            delta2,
            ic: ic_points,
            coefficients,
            a,
            b1,
            b2,
            c,
            h,
        })
    }
}
