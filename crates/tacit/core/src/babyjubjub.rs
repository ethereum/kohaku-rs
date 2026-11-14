//! `BabyJubJub` in circomlib's twisted Edwards form, `168700·u² + v² = 1 + 168696·u²·v²` over the
//! BN254 scalar field, with `Base8` as the generator of its prime-order subgroup.

use std::sync::OnceLock;

use ark_ff::{AdditiveGroup, BigInteger, Field, MontFp, PrimeField};

use crate::field::{Fr, Scalar};

/// A point of `BabyJubJub` in affine coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Point {
    pub x: Fr,
    pub y: Fr,
}

const A: Fr = MontFp!("168700");
const D: Fr = MontFp!("168696");

/// The generator of the prime-order subgroup circomlib signs with.
pub const BASE8: Point = Point {
    x: MontFp!("5299619240641551281634865583518297030282874472190772894086521144482721001553"),
    y: MontFp!("16950150798460657717958625567821834550301663161624707787222815936182638968203"),
};

pub const IDENTITY: Point = Point {
    x: Fr::ZERO,
    y: Fr::ONE,
};

#[derive(Clone, Copy)]
struct Projective {
    x: Fr,
    y: Fr,
    z: Fr,
}

impl Projective {
    const ZERO: Self = Self {
        x: Fr::ZERO,
        y: Fr::ONE,
        z: Fr::ONE,
    };

    fn from(p: Point) -> Self {
        Self {
            x: p.x,
            y: p.y,
            z: Fr::ONE,
        }
    }

    /// add-2008-bbjlp: complete on this curve, so it needs no special cases. The names are the
    /// formula's.
    #[allow(clippy::many_single_char_names)]
    fn add(self, q: Self) -> Self {
        let a = self.z * q.z;
        let b = a.square();
        let c = self.x * q.x;
        let d = self.y * q.y;
        let e = D * c * d;
        let f = b - e;
        let g = b + e;
        Self {
            x: a * f * ((self.x + self.y) * (q.x + q.y) - c - d),
            y: a * g * (d - A * c),
            z: f * g,
        }
    }

    fn affine(self) -> Point {
        let inv = self
            .z
            .inverse()
            .expect("complete addition keeps z non-zero");
        Point {
            x: self.x * inv,
            y: self.y * inv,
        }
    }
}

impl Point {
    #[must_use]
    pub fn add(self, other: Self) -> Self {
        Projective::from(self).add(Projective::from(other)).affine()
    }

    /// `k·self` for a non-negative integer `k`, given as little-endian bits.
    fn mul_bits(self, bits: impl Iterator<Item = bool>) -> Self {
        let mut acc = Projective::ZERO;
        let mut step = Projective::from(self);
        for bit in bits {
            if bit {
                acc = acc.add(step);
            }
            step = step.add(step);
        }
        acc.affine()
    }

    #[cfg(test)]
    #[must_use]
    pub fn mul(self, k: Scalar) -> Self {
        self.mul_bits(k.into_bigint().to_bits_le().into_iter())
    }

    #[must_use]
    pub fn is_identity(&self) -> bool {
        *self == IDENTITY
    }

    #[must_use]
    pub fn is_on_curve(&self) -> bool {
        let (x2, y2) = (self.x.square(), self.y.square());
        A * x2 + y2 == Fr::ONE + D * x2 * y2
    }

    /// Whether the point is in the prime-order subgroup: `L·P` is the identity.
    #[must_use]
    pub fn in_subgroup(&self) -> bool {
        self.is_on_curve()
            && self
                .mul_bits(Scalar::MODULUS.to_bits_le().into_iter())
                .is_identity()
    }

    /// circomlib's `packPoint`: `y` little-endian, the sign of `x` in the top bit.
    #[must_use]
    pub fn pack(&self) -> [u8; 32] {
        let mut out = [0u8; 32];
        out.copy_from_slice(&self.y.into_bigint().to_bytes_le());
        if self.x.into_bigint() > Fr::MODULUS_MINUS_ONE_DIV_TWO {
            out[31] |= 0x80;
        }
        out
    }

    /// The inverse of [`Point::pack`]; `None` unless the bytes name a point of the subgroup.
    #[must_use]
    pub fn unpack(bytes: &[u8; 32]) -> Option<Self> {
        let mut le = *bytes;
        let negative = le[31] & 0x80 != 0;
        le[31] &= 0x7f;
        let y = Fr::from_bigint(ark_ff::BigInt::<4>(std::array::from_fn(|i| {
            u64::from_le_bytes(le[8 * i..8 * i + 8].try_into().expect("8 bytes"))
        })))?;
        let y2 = y.square();
        let den = A - D * y2;
        let mut x = ((Fr::ONE - y2) * den.inverse()?).sqrt()?;
        if (x.into_bigint() > Fr::MODULUS_MINUS_ONE_DIV_TWO) != negative {
            x = -x;
        }
        let point = Self { x, y };
        point.in_subgroup().then_some(point)
    }
}

/// `k·Base8`, from a table of doublings built on first use.
#[must_use]
pub fn mul_base8(k: Scalar) -> Point {
    static TABLE: OnceLock<Vec<Projective>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut table = vec![Projective::from(BASE8)];
        for i in 1..253 {
            table.push(table[i - 1].add(table[i - 1]));
        }
        table
    });
    let mut acc = Projective::ZERO;
    for (i, bit) in k.into_bigint().to_bits_le().into_iter().enumerate() {
        if bit {
            acc = acc.add(table[i]);
        }
    }
    acc.affine()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base8_is_a_subgroup_point() {
        assert!(BASE8.is_on_curve());
        assert!(BASE8.in_subgroup());
        assert_eq!(mul_base8(Scalar::from(1u8)), BASE8);
    }

    #[test]
    fn table_matches_double_and_add() {
        for k in [1u64, 2, 12345, 987_654_321] {
            let k = Scalar::from(k);
            assert_eq!(mul_base8(k), BASE8.mul(k));
        }
        assert!(mul_base8(Scalar::ZERO).is_identity());
    }

    #[test]
    fn packs_losslessly() {
        for k in [1u64, 2, 987_654_321, u64::MAX] {
            let p = mul_base8(Scalar::from(k));
            assert_eq!(Point::unpack(&p.pack()), Some(p));
        }
        assert_eq!(Point::unpack(&[0xff; 32]), None);
    }
}
