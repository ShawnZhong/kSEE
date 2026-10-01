//! A program's line, written as in C: what fits at the cursor (the
//! Machine's completions), the call's signature (its parameter at the
//! cursor in bold), and, as Enter makes it, why it cannot be, if not.

use dioxus::prelude::*;
use ksee_core::program::{context, Completion};
use wasm_bindgen::JsCast;

use crate::machine;

/// The cursor in the input that has the focus (UTF-16 units).
fn cursor() -> Option<usize> {
    let e = web_sys::window()?.document()?.active_element()?.dyn_into::<web_sys::HtmlTextAreaElement>().ok()?;
    e.selection_start().ok().flatten().map(|n| n as usize)
}

/// Line `line` of the program (the names it sees are the lines' before
/// it), starting as `text`; `done(text)` as Enter makes it (empty: the
/// line removed), unless the Machine says it cannot be made; `focus`, as
/// it gets the focus.
#[component]
pub fn Editor(text: String, line: usize, placeholder: String, common: Vec<String>, done: EventHandler<String>, focus: EventHandler<()>) -> Element {
    let mut draft = use_signal(|| text.clone());
    let mut at = use_signal(|| text.len());
    let mut sel = use_signal(|| 0usize);
    let mut open = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let items: Vec<Completion> = if open() { machine().complete(&draft(), at(), line, &common) } else { Vec::new() };
    // (Nothing to choose, or only what is written already: no list.)
    let c = context(&draft(), at());
    let shown = !items.is_empty() && !(items.len() == 1 && items[0].label == if c.param.is_none() { c.call.clone() } else { c.word.clone() });
    let sel_now = sel().min(items.len().saturating_sub(1));
    let hint = (!c.call.is_empty()).then(|| machine().params(&c.call)).flatten();
    let mut take = move |it: Completion| {
        let (d, a) = (draft(), at().min(draft().len()));
        let c = context(&d, a);
        let start = if it.name { c.call_start } else { c.word_start }.min(a);
        let t = format!("{}{}{}", &d[..start], it.text, &d[a..]);
        at.set(start + it.text.len());
        draft.set(t);
        sel.set(0);
    };
    let items2 = items.clone();
    let original = text.clone();
    rsx! {
        div { class: "editor",
            textarea { class: "mono", rows: 1, value: "{draft}", placeholder: "{placeholder}", spellcheck: false, autocomplete: "off",
                oninput: move |e| { draft.set(e.value()); at.set(cursor().unwrap_or(e.value().len())); open.set(true); error.set(None); },
                onclick: move |_| at.set(cursor().unwrap_or(0)),
                onfocus: move |_| { open.set(true); focus.call(()); },
                onblur: move |_| open.set(false),
                onkeydown: move |e| {
                    let key = e.key();
                    if shown && (key == Key::ArrowDown || key == Key::ArrowUp) {
                        e.prevent_default();
                        let n = items2.len();
                        sel.set(if key == Key::ArrowDown { (sel_now + 1) % n } else { (sel_now + n - 1) % n });
                    } else if shown && (key == Key::Tab || key == Key::Enter) {
                        e.prevent_default();
                        take(items2[sel_now].clone());
                    } else if key == Key::Enter {
                        e.prevent_default();
                        let t = draft().trim().to_string();
                        match t.is_empty().then_some(Ok(())).unwrap_or_else(|| machine().can_make(&t, line)) {
                            Ok(()) => { open.set(false); done.call(t); }
                            Err(why) => error.set(Some(why)),
                        }
                    } else if key == Key::Escape {
                        draft.set(original.clone());
                        open.set(false);
                        error.set(None);
                    }
                },
            }
            if shown {
                ul { class: "complete",
                    for (k , it) in items.into_iter().enumerate() {
                        li { class: if k == sel_now { "on" } else { "" },
                            onmousedown: move |e| { e.prevent_default(); take(it.clone()); },
                            span { class: "mono", "{it.label}" } " " span { class: "dim", "{it.detail}" }
                        }
                    }
                }
            }
            if open() && let Some(ps) = hint {
                div { class: "hint mono dim", "{c.call}("
                    for (k , p) in ps.into_iter().enumerate() {
                        if k > 0 { ", " }
                        if Some(k) == c.param { b { "{p}" } } else { "{p}" }
                    }
                    ")"
                }
            }
            if let Some(why) = error() { div { class: "error", "{why}" } }
        }
    }
}
