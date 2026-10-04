//! Web APIs, as JS values.

use js_sys::Reflect;
use wasm_bindgen::JsValue;

pub fn get(obj: &JsValue, key: &str) -> JsValue {
    Reflect::get(obj, &key.into()).unwrap_or(JsValue::UNDEFINED)
}

pub fn set(obj: &JsValue, key: &str, value: impl Into<JsValue>) {
    let _ = Reflect::set(obj, &key.into(), &value.into());
}

/// `WebAssembly.name` (JSPI's Suspending and promising, newer than
/// js_sys's bindings).
pub fn webassembly(name: &str) -> JsValue {
    get(&get(&js_sys::global(), "WebAssembly"), name)
}

/// `ms` milliseconds passed: the page's event loop has run meanwhile.
pub async fn sleep(ms: i32) {
    let p = js_sys::Promise::new(&mut |ok, _| {
        let set: js_sys::Function = wasm_bindgen::JsCast::unchecked_into(get(&js_sys::global(), "setTimeout"));
        let _ = set.call2(&JsValue::NULL, &ok, &ms.into());
    });
    let _ = wasm_bindgen_futures::JsFuture::from(p).await;
}
