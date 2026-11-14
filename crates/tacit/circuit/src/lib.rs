#![doc = include_str!("../README.md")]

use ark_bn254::{Fr, G1Affine, G2Affine};
use ark_ec::AffineRepr;
use ark_ff::PrimeField;
use rand_core::CryptoRng;
use ruint::{ToFieldError, aliases::U256};
use sha2::{Digest, Sha256};

mod groth16;
mod inputs;
mod witness;
mod zkey;

pub use inputs::{CircuitInputs, TREE_DEPTH};

/// The pins on the pool's proving artifacts, the output of its public trusted-setup ceremony.
///
/// Any copy of a file is accepted only if its hash matches, so the mirrors need not be trusted.
pub mod artifacts {
    /// SHA-256 of `transact.wasm`, the circuit's witness program.
    pub const WASM_SHA256: [u8; 32] =
        hex32("02dd5e84970e5bc629a7a3cd4d7eae5fc9ca05579fa22c9b39b5ea70c8d8a6c1");
    /// SHA-256 of `transact_final.zkey`, the proving key.
    pub const ZKEY_SHA256: [u8; 32] =
        hex32("40758061a0786fb0bdc5e5dec4c354bbf85fc106f7412716e25e781af4e79c4b");
    /// SHA-256 over the verification key's content (see [`crate::Prover::vk_hash`]): the key the
    /// pool's verifier contract checks proofs with.
    pub const VK_HASH: [u8; 32] =
        hex32("43d11e6e1607e1ea7f3980c9bca91beed95e2e80d173d0873189e99d402e5757");

    /// Where `transact.wasm` (about 4.9 MB) can be downloaded.
    pub const WASM_URLS: &[&str] = &[
        "https://tacit.finance/evm-pool/transact.wasm",
        "https://ipfs.io/ipfs/bafybeia7b7euebs6dr7muhhvwf4yh472ftxodikyecxubipam4d7o7sxta",
    ];
    /// Where `transact_final.zkey` (about 28.6 MB) can be downloaded.
    pub const ZKEY_URLS: &[&str] = &[
        "https://tacit.finance/evm-pool/transact_final.zkey",
        "https://ipfs.io/ipfs/bafybeia6c36bgww2svm6jfg6nl2pk337prc7t6gcfsvhuo5sufqjpi7z4a",
    ];

    const fn hex32(s: &str) -> [u8; 32] {
        let s = s.as_bytes();
        let mut out = [0u8; 32];
        let mut i = 0;
        while i < 32 {
            out[i] = (nibble(s[2 * i]) << 4) | nibble(s[2 * i + 1]);
            i += 1;
        }
        out
    }

    const fn nibble(c: u8) -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            _ => panic!("not a lowercase hex digit"),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CircuitError {
    #[error("{0} does not match its pinned hash")]
    PinMismatch(&'static str),
    #[error("invalid proving key: {0}")]
    ProvingKey(&'static str),
    #[error("witness program error: {0}")]
    WitnessProgram(String),
    #[error("the circuit rejected the inputs: {0}")]
    Witness(String),
    #[error("input signal {signal} takes {expected} values, got {actual}")]
    InputSize {
        signal: String,
        expected: usize,
        actual: usize,
    },
    #[error("an input is not a field element: {0}")]
    ToField(#[from] ToFieldError),
    #[error("the witness's public signals are not the inputs'")]
    PublicSignalsMismatch,
    #[error("a fresh proof does not verify")]
    ProofDoesNotVerify,
}

/// A Groth16 proof.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Proof {
    pub a: G1Affine,
    pub b: G2Affine,
    pub c: G1Affine,
}

impl Proof {
    /// `pA` as the pool's `transact` takes it.
    #[must_use]
    pub fn pa(&self) -> [U256; 2] {
        g1_words(&self.a)
    }

    /// `pB` as the pool's `transact` takes it: each coordinate's pair swapped, `[[x.c1, x.c0],
    /// [y.c1, y.c0]]`.
    #[must_use]
    pub fn pb(&self) -> [[U256; 2]; 2] {
        let (x, y) = self.b.xy().unwrap_or_default();
        [[word(x.c1), word(x.c0)], [word(y.c1), word(y.c0)]]
    }

    /// `pC` as the pool's `transact` takes it.
    #[must_use]
    pub fn pc(&self) -> [U256; 2] {
        g1_words(&self.c)
    }
}

fn g1_words(point: &G1Affine) -> [U256; 2] {
    let (x, y) = point.xy().unwrap_or_default();
    [word(x), word(y)]
}

fn word(x: impl PrimeField<BigInt = ark_ff::BigInt<4>>) -> U256 {
    U256::from_limbs(x.into_bigint().0)
}

/// The pool's prover: the witness program and the proving key, checked against their pins.
pub struct Prover {
    program: witness::WitnessProgram,
    key: zkey::ProvingKey,
}

impl Prover {
    /// Loads the prover from the bytes of `transact.wasm` and `transact_final.zkey`.
    ///
    /// # Errors
    /// Returns an error if either file differs from its pin, or cannot be read.
    pub fn new(wasm: &[u8], zkey: &[u8]) -> Result<Self, CircuitError> {
        if <[u8; 32]>::from(Sha256::digest(wasm)) != artifacts::WASM_SHA256 {
            return Err(CircuitError::PinMismatch("transact.wasm"));
        }
        if <[u8; 32]>::from(Sha256::digest(zkey)) != artifacts::ZKEY_SHA256 {
            return Err(CircuitError::PinMismatch("transact_final.zkey"));
        }
        let key = zkey::ProvingKey::read(zkey)?;
        if groth16::vk_hash(&key) != artifacts::VK_HASH {
            return Err(CircuitError::PinMismatch("the verification key"));
        }
        let program = witness::WitnessProgram::new(wasm)?;
        Ok(Self { program, key })
    }

    /// The content hash of the verification key this prover's proofs verify against.
    #[must_use]
    pub fn vk_hash(&self) -> [u8; 32] {
        groth16::vk_hash(&self.key)
    }

    /// Proves `inputs`. The proof is checked against the verification key before it is returned, so
    /// a proving fault surfaces here rather than as a reverted transaction.
    ///
    /// # Errors
    /// Returns an error if the inputs do not satisfy the circuit, or the proof does not verify.
    pub fn prove(
        &self,
        inputs: &CircuitInputs,
        rng: &mut impl CryptoRng,
    ) -> Result<Proof, CircuitError> {
        let witness = self.witness(inputs)?;
        let r = random_scalar(rng);
        let s = random_scalar(rng);
        let proof = groth16::prove(&self.key, &witness, r, s)?;
        if !groth16::verify(&self.key, &proof, &witness[1..=self.key.n_public]) {
            return Err(CircuitError::ProofDoesNotVerify);
        }
        Ok(proof)
    }

    /// Checks `proof` for `public_signals`.
    #[must_use]
    pub fn verify(&self, proof: &Proof, public_signals: &[U256; 11]) -> bool {
        let Ok(public) = public_signals
            .iter()
            .map(|x| Fr::try_from(*x))
            .collect::<Result<Vec<_>, _>>()
        else {
            return false;
        };
        groth16::verify(&self.key, proof, &public)
    }

    /// The full witness for `inputs`, its public signals checked against the inputs'.
    fn witness(&self, inputs: &CircuitInputs) -> Result<Vec<Fr>, CircuitError> {
        let signals = inputs
            .signals()
            .into_iter()
            .map(|(name, values)| {
                let values = values
                    .into_iter()
                    .map(Fr::try_from)
                    .collect::<Result<Vec<_>, _>>()?;
                Ok((name, values))
            })
            .collect::<Result<Vec<_>, CircuitError>>()?;
        let witness = self.program.calculate(&signals)?;
        let expected = inputs.public_signals();
        if witness.len() <= expected.len()
            || witness[1..=expected.len()]
                .iter()
                .zip(&expected)
                .any(|(w, x)| word(*w) != *x)
        {
            return Err(CircuitError::PublicSignalsMismatch);
        }
        Ok(witness)
    }
}

fn random_scalar(rng: &mut impl CryptoRng) -> Fr {
    let mut bytes = [0u8; 64];
    rng.fill_bytes(&mut bytes);
    Fr::from_le_bytes_mod_order(&bytes)
}
