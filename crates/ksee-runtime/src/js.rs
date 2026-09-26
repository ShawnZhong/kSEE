//! Web APIs, reached the same way from the page and Deno.

use js_sys::{Reflect, Uint8Array};
use wasm_bindgen::JsValue;

pub fn get(obj: &JsValue, key: &str) -> JsValue {
    Reflect::get(obj, &key.into()).unwrap_or(JsValue::UNDEFINED)
}

pub fn set(obj: &JsValue, key: &str, value: impl Into<JsValue>) {
    let _ = Reflect::set(obj, &key.into(), &value.into());
}

/// Our memory at `p..p+len` as bytes owned by JS (a copy).
pub fn copy_from(p: *const u8, len: u32) -> Uint8Array {
    let src = unsafe { core::slice::from_raw_parts(p, len as usize) };
    Uint8Array::from(src)
}

/// A Rust closure as a JS function (kept alive: the kernel's imports live
/// as long as the page).
pub fn func<F: ?Sized + wasm_bindgen::closure::WasmClosure>(c: wasm_bindgen::closure::Closure<F>) -> JsValue {
    c.into_js_value()
}
