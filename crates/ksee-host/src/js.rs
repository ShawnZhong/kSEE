//! Web APIs, as JS values.

use js_sys::Reflect;
use wasm_bindgen::JsValue;

pub fn get(obj: &JsValue, key: &str) -> JsValue {
    Reflect::get(obj, &key.into()).unwrap_or(JsValue::UNDEFINED)
}

pub fn set(obj: &JsValue, key: &str, value: impl Into<JsValue>) {
    let _ = Reflect::set(obj, &key.into(), &value.into());
}
