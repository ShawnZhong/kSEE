//! The source where the cursor is: the function of the frame shown, its
//! lines, the one it runs marked (the kernel's sources, as the build copied
//! them to the site's src/).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use dioxus::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use crate::machine;
use crate::state::Ui;

thread_local! {
    static FILES: RefCell<BTreeMap<String, Rc<Vec<String>>>> = const { RefCell::new(BTreeMap::new()) };
}

/// The lines of source file `file`, fetched once.
async fn file(file: String) -> Option<Rc<Vec<String>>> {
    if let Some(f) = FILES.with_borrow(|f| f.get(&file).cloned()) {
        return Some(f);
    }
    let w = web_sys::window()?;
    let r: web_sys::Response = JsFuture::from(w.fetch_with_str(&format!("src/{file}"))).await.ok()?.dyn_into().ok()?;
    if !r.ok() {
        return None;
    }
    let text = JsFuture::from(r.text().ok()?).await.ok()?.as_string()?;
    let lines = Rc::new(text.lines().map(String::from).collect::<Vec<_>>());
    FILES.with_borrow_mut(|f| f.insert(file, lines.clone()));
    Some(lines)
}

#[component]
pub fn Source() -> Element {
    let ui = use_context::<Ui>();
    let stack = crate::stack::frames();
    let Some(f) = crate::stack::shown(ui, &stack.0 .0) else { return rsx! {} };
    let (Some(path), Some(line)) = (f.file.clone(), f.line) else { return rsx! {} };
    // (From the function's first line (where its file has it: else ten
    // before the line) to its last, or ten past the line.)
    let def = machine().view(|v| v.definition(&f.func)).filter(|d| d.file == path && d.line <= line);
    let from = def.as_ref().map_or(line.saturating_sub(10), |d| d.line).max(1);
    let to = def.and_then(|d| d.last).unwrap_or(0).max(line + 10);
    rsx! {
        div { class: "source",
            div { class: "srchead mono dim", "{path}" }
            Code { key: "{path}", path: path.clone(), from, to, line }
        }
    }
}

/// File `path`'s lines `from..=to`, line `line` marked (and in view).
#[component]
fn Code(path: String, from: u32, to: u32, line: u32) -> Element {
    let text = use_resource(move || file(path.clone()));
    let Some(all) = text.read().clone().flatten() else { return rsx! { p { class: "dim pad", "…" } } };
    rsx! {
        div { class: "code mono",
            for n in from..=to.min(all.len() as u32) {
                div { key: "{n}", class: if n == line { "ln now" } else { "ln" },
                    onmounted: move |e| async move { if n == line { let _ = e.scroll_to(ScrollBehavior::Smooth).await; } },
                    span { class: "no", "{n}" }
                    "{all[n as usize - 1]}"
                }
            }
        }
    }
}
