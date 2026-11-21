use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "bigint")]
    pub type JsU64;
    #[wasm_bindgen(typescript_type = "bigint")]
    pub type JsU128;
}

impl TryFrom<JsU64> for u64 {
    type Error = JsError;

    fn try_from(value: JsU64) -> Result<Self, Self::Error> {
        Self::try_from(JsValue::from(value))
            .map_err(|_| JsError::new("value must be a bigint in the u64 range"))
    }
}

impl From<u64> for JsU64 {
    fn from(value: u64) -> Self {
        JsValue::from(value).unchecked_into()
    }
}

impl TryFrom<JsU128> for u128 {
    type Error = JsError;

    fn try_from(value: JsU128) -> Result<Self, Self::Error> {
        Self::try_from(JsValue::from(value))
            .map_err(|_| JsError::new("value must be a bigint in the u128 range"))
    }
}

impl From<u128> for JsU128 {
    fn from(value: u128) -> Self {
        JsValue::from(value).unchecked_into()
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_tests {
    use super::*;
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    fn bigint_inputs_are_checked() {
        assert_eq!(u64::try_from(JsU64::from(1u64)).unwrap(), 1);
        assert_eq!(u128::try_from(JsU128::from(u128::MAX)).unwrap(), u128::MAX);
        assert!(
            u64::try_from(
                JsValue::bigint_from_str("18446744073709551616").unchecked_into::<JsU64>()
            )
            .is_err()
        );
        assert!(u128::try_from(JsValue::from_f64(1.0).unchecked_into::<JsU128>()).is_err());
    }
}
