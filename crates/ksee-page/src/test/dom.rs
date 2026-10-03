use js_sys::Promise;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Document, Element, Event, EventInit, HtmlElement, HtmlTextAreaElement, KeyboardEvent, KeyboardEventInit,
              MouseEvent, MouseEventInit, PointerEvent, PointerEventInit};

pub(super) fn doc() -> Document {
    web_sys::window().and_then(|w| w.document()).expect("a document")
}

pub(super) fn all(sel: &str) -> Vec<Element> {
    let list = doc().query_selector_all(sel).expect("a selector");
    (0..list.length()).filter_map(|k| list.item(k)?.dyn_into().ok()).collect()
}

pub(super) fn one(sel: &str) -> Option<Element> {
    doc().query_selector(sel).ok().flatten()
}

pub(super) fn text(e: &Element) -> String {
    e.text_content().unwrap_or_default()
}

/// The first of `sel` whose text is `t`.
pub(super) fn named(sel: &str, t: &str) -> Option<Element> {
    all(sel).into_iter().find(|e| text(e) == t)
}

pub(super) async fn sleep(ms: i32) {
    let p = Promise::new(&mut |done, _| {
        let _ = web_sys::window().unwrap().set_timeout_with_callback_and_timeout_and_arguments_0(&done, ms);
    });
    let _ = JsFuture::from(p).await;
}

/// Wait until `f` holds, checking every 100 ms, `n` times at most.
pub(super) async fn until(f: impl Fn() -> bool, n: usize) -> bool {
    for _ in 0..n {
        if f() {
            return true;
        }
        sleep(100).await;
    }
    f()
}

pub(super) fn click(e: &Element) {
    if let Some(h) = e.dyn_ref::<HtmlElement>() {
        h.click();
    }
}

pub(super) fn mouse(e: &Element, kind: &str, x: f64, y: f64, shift: bool) {
    let init = MouseEventInit::new();
    init.set_bubbles(true);
    init.set_client_x(x as i32);
    init.set_client_y(y as i32);
    init.set_shift_key(shift);
    let _ = e.dispatch_event(&MouseEvent::new_with_mouse_event_init_dict(kind, &init).unwrap());
}

pub(super) fn pointer(e: &Element, kind: &str, x: f64, y: f64) {
    let init = PointerEventInit::new();
    init.set_bubbles(true);
    init.set_client_x(x as i32);
    init.set_client_y(y as i32);
    let _ = e.dispatch_event(&PointerEvent::new_with_event_init_dict(kind, &init).unwrap());
}

pub(super) fn key(e: &Element, k: &str) {
    let init = KeyboardEventInit::new();
    init.set_bubbles(true);
    init.set_key(k);
    let _ = e.dispatch_event(&KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init).unwrap());
}

/// A key handled by the page's global window listener, independent of which
/// form control the preceding test interaction left focused.
pub(super) fn global_key(k: &str) {
    if let Some(active) = doc().active_element().and_then(|e| e.dyn_into::<HtmlElement>().ok()) {
        let _ = active.blur();
    }
    let init = KeyboardEventInit::new();
    init.set_bubbles(true);
    init.set_key(k);
    let _ = web_sys::window().unwrap().dispatch_event(&KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init).unwrap());
}

pub(super) fn input(e: &Element) {
    let init = EventInit::new();
    init.set_bubbles(true);
    let _ = e.dispatch_event(&Event::new_with_event_init_dict("input", &init).unwrap());
}

/// The program's lines (not the one that adds a line): their text and what each did.
pub(super) fn cells() -> Vec<(String, String)> {
    all("ol.steps > li:not(.add) .cell").iter().map(|c| {
        let t = c.query_selector("textarea").ok().flatten().and_then(|t| t.dyn_into::<HtmlTextAreaElement>().ok()).map(|t| t.value());
        let r = c.query_selector(".ret").ok().flatten().map(|r| text(&r));
        (t.unwrap_or_default(), r.unwrap_or_default())
    }).collect()
}

/// The first `n` lines made, each with what it did.
pub(super) fn done(n: usize) -> bool {
    let c = cells();
    c.len() >= n && c[..n].iter().all(|(_, r)| !r.is_empty() && r != "…")
}

/// Where the cursor is, as the transport says ("task · func file:line · in call"): "func file:line".
pub(super) fn place() -> String {
    one(".transport .where").map(|w| text(&w)).and_then(|t| t.split(" · ").nth(1).map(String::from)).unwrap_or_default()
}

/// A click on a transport button (Shift held: over the calls).
pub(super) fn press(label: &str, shift: bool) {
    if let Some(b) = named(".transport button", label) {
        mouse(&b, "click", 0.0, 0.0, shift);
    }
}

/// A line's text changed and Enter pressed.
pub(super) fn write(ta: &Element, t: &str) {
    let Some(a) = ta.dyn_ref::<HtmlTextAreaElement>() else { return };
    let _ = a.focus();
    a.set_value(t);
    let _ = a.set_selection_range(t.len() as u32, t.len() as u32);
    input(ta);
    key(ta, "Enter");
}

pub(super) fn fail(what: &str, detail: impl std::fmt::Debug) -> String {
    format!("{what}: {detail:?}")
}

/// Check `ok`, else fail saying `what` and `detail`.
pub(super) fn check(ok: bool, what: &str, detail: impl std::fmt::Debug) -> Result<(), String> {
    if ok { Ok(()) } else { Err(fail(what, detail)) }
}

/// A tab chosen.
pub(super) async fn tab(name: &str) {
    if let Some(t) = named(".tabs .tab", name) {
        click(&t);
    }
    sleep(300).await;
}

/// Wait for `f`, at most `n` checks; then check it.
pub(super) async fn wait(f: impl Fn() -> bool, n: usize, what: &str, detail: impl Fn() -> String) -> Result<(), String> {
    let ok = until(f, n).await;
    check(ok, what, detail())
}
