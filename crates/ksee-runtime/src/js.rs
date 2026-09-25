//! Web APIs, reached the same way from the page and Workers (and Deno).

use js_sys::{Reflect, Uint8Array};
use wasm_bindgen::JsValue;

pub fn get(obj: &JsValue, key: &str) -> JsValue {
    Reflect::get(obj, &key.into()).unwrap_or(JsValue::UNDEFINED)
}

pub fn set(obj: &JsValue, key: &str, value: impl Into<JsValue>) {
    let _ = Reflect::set(obj, &key.into(), &value.into());
}

/// Copy `len` bytes of an ArrayBuffer-backed view into our memory at `dst`.
pub fn copy_to(src: &Uint8Array, dst: *mut u8) {
    let dst = unsafe { core::slice::from_raw_parts_mut(dst, src.length() as usize) };
    src.copy_to(dst);
}

/// Our memory at `p..p+len` as bytes owned by JS (a copy).
pub fn copy_from(p: *const u8, len: u32) -> Uint8Array {
    let src = unsafe { core::slice::from_raw_parts(p, len as usize) };
    Uint8Array::from(src)
}
