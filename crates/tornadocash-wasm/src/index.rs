use wasm_bindgen::{JsError, JsValue};

/// Decode a tree index before passing tree bounds and capacity checks to the core.
#[allow(dead_code, reason = "shared helper for Merkle tree bindings")]
pub(crate) fn tree_index(value: &JsValue) -> Result<u32, JsError> {
    let message = "Index must be a finite integer number in the u32 range";
    let value = value.as_f64().ok_or_else(|| JsError::new(message))?;
    if !value.is_finite() || value.fract() != 0.0 || !(0.0..=f64::from(u32::MAX)).contains(&value) {
        return Err(JsError::new(message));
    }

    #[allow(
        clippy::cast_sign_loss,
        reason = "validated as an integer in the u32 range"
    )]
    Ok(value as u32)
}

#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_tests {
    use wasm_bindgen::JsValue;
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::tree_index;

    #[wasm_bindgen_test]
    fn accepts_u32_index_boundaries() {
        for expected in [0, u32::MAX] {
            assert_eq!(tree_index(&JsValue::from(expected)).unwrap(), expected);
        }
    }

    #[wasm_bindgen_test]
    fn rejects_invalid_numeric_indices() {
        for value in [
            -1.0,
            f64::from(u32::MAX) + 1.0,
            0.5,
            f64::NAN,
            f64::INFINITY,
        ] {
            assert!(tree_index(&JsValue::from_f64(value)).is_err());
        }
    }

    #[wasm_bindgen_test]
    fn rejects_bigint_index() {
        assert!(tree_index(&JsValue::from(1_u64)).is_err());
    }
}
