use kohaku_tornadocash::note::Note as CoreNote;
use rand::{
    SeedableRng,
    rngs::{StdRng, SysRng},
};
use serde::{Deserialize, Deserializer, Serialize, de::Error};
use tsify::serde_wasm_bindgen::preserve;
use tsify::{Ts, Tsify};
use wasm_bindgen::prelude::*;

/// Structured note data, including the original nullifier and secret bytes.
#[derive(Serialize, Deserialize, Tsify)]
#[serde(rename_all = "camelCase")]
#[tsify(large_number_types_as_bigints)]
pub struct NoteData {
    pub symbol: String,
    pub amount: String,
    #[serde(deserialize_with = "deserialize_chain_id")]
    pub chain_id: u64,
    #[serde(
        serialize_with = "serde_bytes::serialize",
        deserialize_with = "deserialize_bytes31"
    )]
    #[tsify(type = "Uint8Array")]
    pub nullifier: [u8; 31],
    #[serde(
        serialize_with = "serde_bytes::serialize",
        deserialize_with = "deserialize_bytes31"
    )]
    #[tsify(type = "Uint8Array")]
    pub secret: [u8; 31],
}

// Preserve each JS value until its type is checked, before Serde can coerce it.
fn deserialize_chain_id<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
    let value: JsValue = preserve::deserialize(deserializer)?;
    u64::try_from(value).map_err(|_| D::Error::custom("chainId must be a bigint in the u64 range"))
}

fn deserialize_bytes31<'de, D: Deserializer<'de>>(deserializer: D) -> Result<[u8; 31], D::Error> {
    let value: JsValue = preserve::deserialize(deserializer)?;
    let bytes = value
        .dyn_into::<js_sys::Uint8Array>()
        .map_err(|_| D::Error::custom("note bytes must be a Uint8Array"))?;
    if bytes.length() != 31 {
        return Err(D::Error::custom("note bytes must have length 31"));
    }
    let mut result = [0; 31];
    bytes.copy_to(&mut result);
    Ok(result)
}

/// A Tornado note stored in WASM memory until its JavaScript wrapper is freed.
#[wasm_bindgen]
pub struct Note {
    inner: CoreNote,
}

#[wasm_bindgen]
impl Note {
    /// Construct a note from structured data using the core constructor.
    ///
    /// # Errors
    ///
    /// Throws a JavaScript `Error` if fields cannot be converted to Rust.
    /// Secrets must be 31-byte `Uint8Array`s and the chain ID a `bigint` in `u64`.
    #[wasm_bindgen(constructor)]
    pub fn new(data: &Ts<NoteData>) -> Result<Note, JsError> {
        let data = data.to_rust()?;
        Ok(Self {
            inner: CoreNote::new(
                data.nullifier,
                data.secret,
                data.symbol,
                data.amount,
                data.chain_id,
            ),
        })
    }

    /// Generate a fresh note using the environment's cryptographic randomness.
    ///
    /// # Errors
    ///
    /// Throws a JavaScript `Error` if the chain ID is not a `bigint` in the
    /// `u64` range or the environment cannot provide a secure random seed.
    pub fn random(symbol: &str, amount: &str, chain_id: js_sys::BigInt) -> Result<Note, JsError> {
        let chain_id = u64::try_from(chain_id)
            .map_err(|_| JsError::new("chainId must be a bigint in the u64 range"))?;
        let mut rng = StdRng::try_from_rng(&mut SysRng)?;
        Ok(Self {
            inner: CoreNote::random(symbol, amount, chain_id, &mut rng),
        })
    }

    /// Parse a legacy note and retain the resulting core instance.
    ///
    /// # Errors
    ///
    /// Throws a JavaScript `Error` with the core parser's message on failure.
    pub fn parse(text: &str) -> Result<Note, JsError> {
        Ok(Self {
            inner: text.parse()?,
        })
    }

    /// Export an independent copy of the note's fields, including its secrets.
    ///
    /// # Errors
    ///
    /// Throws a JavaScript `Error` if conversion to JavaScript fails.
    #[wasm_bindgen(js_name = toObject)]
    pub fn to_object(&self) -> Result<Ts<NoteData>, JsError> {
        let data = NoteData {
            symbol: self.inner.symbol.clone(),
            amount: self.inner.amount.clone(),
            chain_id: self.inner.chain_id,
            nullifier: self.inner.nullifier.into_bytes(),
            secret: self.inner.secret.into_bytes(),
        };
        Ok(data.into_ts()?)
    }

    /// Format the note using the core's legacy `Display` representation.
    #[wasm_bindgen(js_name = toString)]
    #[must_use]
    pub fn format(&self) -> String {
        self.inner.to_string()
    }

    /// Return a copy of the 62-byte preimage: nullifier followed by secret.
    #[must_use]
    pub fn preimage(&self) -> Vec<u8> {
        self.inner.preimage().to_vec()
    }

    /// Compute the commitment as `0x` followed by 64 lowercase hex digits.
    #[must_use]
    pub fn commitment(&self) -> String {
        format!("0x{:064x}", self.inner.commitment())
    }

    /// Compute the nullifier hash as `0x` followed by 64 lowercase hex digits.
    ///
    /// This does not check whether the note has been spent.
    #[wasm_bindgen(js_name = nullifierHash)]
    #[must_use]
    pub fn nullifier_hash(&self) -> String {
        format!("0x{:064x}", self.inner.nullifier_hash())
    }
}
