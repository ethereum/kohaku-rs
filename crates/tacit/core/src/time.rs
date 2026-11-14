//! Waiting between retries, natively and under `wasm32`, where tokio has no timer.

use std::time::Duration;

/// Sleeps for `duration`: with tokio natively, with the host's `setTimeout` under `wasm32`.
#[cfg(not(target_arch = "wasm32"))]
pub async fn sleep(duration: Duration) {
    tokio::time::sleep(duration).await;
}

/// Sleeps for `duration`: with tokio natively, with the host's `setTimeout` under `wasm32`.
#[cfg(target_arch = "wasm32")]
pub async fn sleep(duration: Duration) {
    use wasm_bindgen::{JsCast, JsValue};

    let millis = JsValue::from(f64::from(
        u32::try_from(duration.as_millis()).unwrap_or(u32::MAX),
    ));
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        let global = js_sys::global();
        if let Ok(set_timeout) = js_sys::Reflect::get(&global, &JsValue::from_str("setTimeout"))
            .and_then(|f| f.dyn_into::<js_sys::Function>())
        {
            let _ = set_timeout.call2(&global, &resolve, &millis);
        } else {
            let _ = resolve.call0(&JsValue::UNDEFINED);
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}
