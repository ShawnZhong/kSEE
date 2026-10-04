//! The page's navigation: the header (every lesson, by chapter; where the
//! lesson is; the source; the theme), the lessons before and after at the
//! side's foot, and the URL (the lesson's page, NAME.html, and a changed
//! program, #p=JSON).

use dioxus::prelude::*;

use crate::examples;
use crate::lesson::Step;
use crate::state::Ui;

/// The URL's state: the lesson, its page's name (NAME.html); and #p=JSON,
/// its program, if changed.
fn url() -> (Option<String>, Option<String>) {
    let Some(loc) = web_sys::window().map(|w| w.location()) else { return (None, None) };
    let path = loc.pathname().unwrap_or_default();
    let ex = path.rsplit('/').next().and_then(|f| f.strip_suffix(".html")).map(String::from);
    let p = loc.hash().unwrap_or_default().trim_start_matches('#').split('&').find_map(|kv| kv.strip_prefix("p=").map(String::from))
        .and_then(|v| js_sys::decode_uri_component(&v).ok()?.as_string());
    (ex, p)
}

/// A program as the URL has it: JSON.
fn program_json(program: &[Step]) -> Option<String> {
    let value = serde_wasm_bindgen::to_value(program).ok()?;
    js_sys::JSON::stringify(&value).ok()?.as_string()
}

fn program_from_json(text: &str) -> Option<Vec<Step>> {
    serde_wasm_bindgen::from_value(js_sys::JSON::parse(text).ok()?).ok()
}

/// The lesson and program the URL says, at first.
pub fn from_url(ui: Ui) {
    let (ex, p) = url();
    let all = examples();
    if let Some(k) = ex.and_then(|n| all.iter().position(|e| e.name == n)) {
        let program = p.as_deref().and_then(program_from_json).unwrap_or_else(|| all[k].steps.clone());
        ui.open(k, program);
    }
}

/// The URL, as the lesson and its program are.
pub(crate) fn to_url(lesson: usize, program: &[Step]) {
    let all = examples();
    let Some(ex) = all.get(lesson) else { return };
    let p = if program == ex.steps { String::new() } else {
        program_json(program).map_or_else(String::new, |p| format!("#p={}", js_sys::encode_uri_component(&p)))
    };
    if let Some(h) = web_sys::window().and_then(|w| w.history().ok()) {
        let _ = h.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&format!("{}.html{p}", ex.name)));
    }
}

/// The theme kept in the browser's storage (none: the system's), as the
/// page's head applies it before the page is drawn.
const THEME: &str = "ksee.theme";

/// The theme: the system's, light or dark, kept.
#[component]
fn Theme() -> Element {
    let storage = || web_sys::window().and_then(|w| w.local_storage().ok().flatten());
    let mut theme = use_signal(move || storage().and_then(|s| s.get_item(THEME).ok().flatten()).unwrap_or_default());
    // (One button, round the three: the system's, light, dark.)
    let (label, next) = match theme().as_str() { "light" => ("☀ Light", "dark"), "dark" => ("☾ Dark", ""), _ => ("◐ Auto", "light") };
    rsx! {
        button { class: "theme", title: "the colors: the system's, light or dark (a click: the next)",
            onclick: move |_| {
                let root = web_sys::window().and_then(|w| w.document()).and_then(|d| d.document_element());
                if let (Some(root), Some(s)) = (root, storage()) {
                    let _ = if next.is_empty() { root.remove_attribute("data-theme").and(s.remove_item(THEME)) }
                            else { root.set_attribute("data-theme", next).and(s.set_item(THEME, next)) };
                }
                theme.set(next.into());
            },
            "{label}" }
    }
}

/// The commit the page was built from (+: with changes not committed), and
/// where its source is: `ksee build`'s.
const COMMIT: Option<&str> = option_env!("KSEE_COMMIT");
const SOURCE: Option<&str> = option_env!("KSEE_SOURCE");

#[component]
pub fn Header() -> Element {
    let ui = use_context::<Ui>();
    let mut menu = use_signal(|| false);
    let all = examples();
    let at = (ui.lesson)();
    // (Where the lesson is: its chapter, and which of its lessons.)
    let group: Vec<usize> = (0..all.len()).filter(|&k| all[k].group == all[at].group).collect();
    let place = group.iter().position(|&k| k == at).unwrap_or(0) + 1;
    rsx! {
        header { class: "topbar",
            div { class: "left",
                button { class: if menu() { "contents on" } else { "contents" }, title: "every lesson, by chapter", onclick: move |_| menu.toggle(), "☰ Lessons" }
                span { class: "crumb", "{all[at].group} · {place} of {group.len()}" }
            }
            b { "kSEE" span { class: "dim", " · step through a real Linux kernel" } }
            div { class: "right",
                if let (Some(commit), Some(source)) = (COMMIT, SOURCE) {
                    a { class: "build", href: "{source}/tree/{commit.trim_end_matches('+')}", target: "_blank",
                        title: "the source this page was built from: {commit} (+: with changes not committed)", "source" }
                }
                Theme {}
            }
            if menu() {
                div { class: "menuback", onclick: move |_| menu.set(false) }
                Contents { close: move |_| menu.set(false) }
            }
        }
    }
}

/// The lessons before and after this one, by their titles, a line each
/// (at the side's foot: the menu, at the top, has them all).
#[component]
pub fn Turn() -> Element {
    let ui = use_context::<Ui>();
    let all = examples();
    let at = (ui.lesson)();
    rsx! {
        nav { class: "turn",
            // (Each also its page's link, NAME.html, for what reads the page
            // without running it; a click opens it here.)
            if let Some(e) = at.checked_sub(1).map(|k| &all[k]) {
                a { title: "the lesson before", href: "{e.name}.html",
                    onclick: move |ev: MouseEvent| { ev.prevent_default(); ui.open_lesson(at - 1) }, "◀ {e.title}" }
            }
            if let Some(e) = all.get(at + 1) {
                a { class: "next", title: "the next lesson", href: "{e.name}.html",
                    onclick: move |ev: MouseEvent| { ev.prevent_default(); ui.open_lesson(at + 1) }, "{e.title} ▶" }
            }
        }
    }
}

/// Every lesson, by chapter, with the open one marked.
#[component]
fn Contents(close: EventHandler<()>) -> Element {
    let ui = use_context::<Ui>();
    let all = examples();
    let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
    for (k, e) in all.iter().enumerate() {
        match groups.last_mut() {
            Some((g, ks)) if *g == e.group => ks.push(k),
            _ => groups.push((e.group.clone(), vec![k])),
        }
    }
    rsx! {
        nav { class: "menu",
            for (g , ks) in groups {
                div { class: "chapter", "{g}" }
                for k in ks {
                    a { class: if k == (ui.lesson)() { "item on" } else { "item" },
                        onclick: move |_| { ui.open_lesson(k); close.call(()); },
                        "{all[k].title}" }
                }
            }
        }
    }
}

