//! The page, in Rust (Dioxus), in the runtime's wasm module: it calls the
//! Machine directly. A lesson is a notebook: its program's lines, each
//! with what it did and the paragraphs that explain it, made one after
//! another on a fresh kernel; beside it, the kernel's console.

use std::cell::RefCell;
use std::rc::Rc;

use dioxus::prelude::*;
use ksee_runtime::machine::Machine;
use serde::Deserialize;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

mod text;

thread_local! {
    /// The Machine (one thread: the page's).
    static MACHINE: RefCell<Option<Rc<Machine>>> = const { RefCell::new(None) };
    static EXAMPLES: RefCell<Vec<Example>> = const { RefCell::new(Vec::new()) };
}

fn machine() -> Rc<Machine> {
    MACHINE.with_borrow(|m| m.clone().expect("mounted"))
}

/// A lesson (examples.json): its chapter, title, question, explanation,
/// tries, and its program's lines.
#[derive(Deserialize, Clone, PartialEq)]
pub struct Example {
    pub name: String,
    pub group: String,
    pub title: String,
    pub question: String,
    pub text: Vec<String>,
    #[serde(default)]
    pub tries: Vec<String>,
    pub steps: Vec<String>,
}

/// Mount the page into the element `root`, with the booted `machine` (the
/// page owns it from here) and the lessons, `examples` (examples.json).
#[wasm_bindgen]
pub fn mount(root: &str, machine: Machine, examples: JsValue) -> Result<(), JsValue> {
    let examples: Vec<Example> = serde_wasm_bindgen::from_value(examples)?;
    MACHINE.with_borrow_mut(|m| *m = Some(Rc::new(machine)));
    EXAMPLES.with_borrow_mut(|e| *e = examples);
    dioxus::LaunchBuilder::web().with_cfg(dioxus::web::Config::new().rootname(root)).launch(App);
    Ok(())
}

/// How a line went: a variable declared (as it starts), memory written
/// (what it holds then), or a call: asked, then finished (its result, and
/// during which line), or not yet (it blocks).
#[derive(Clone, PartialEq)]
enum Outcome {
    Declared(String),
    Assigned(String),
    Call { id: u32, ret: Option<f64>, how: Option<String>, during: Option<usize> },
    Failed(String),
}

/// The kernel quiet's news (Machine::quiet).
#[derive(Deserialize)]
struct News {
    finished: Vec<Finished>,
}
#[derive(Deserialize)]
struct Finished {
    id: u32,
    how: String,
    ret: f64,
}

/// Make lesson `lines` on a fresh kernel, one after another, each line's
/// outcome into `out` as it goes; the console's text into `console`.
async fn run(lines: Vec<String>, mut out: Signal<Vec<Outcome>>, mut console: Signal<String>) {
    let m = machine();
    out.write().clear();
    if m.restart().await.is_err() {
        return;
    }
    for (j, line) in lines.iter().enumerate() {
        let made = match m.make(line) {
            Ok(made) => made,
            Err(e) => {
                out.write().push(Outcome::Failed(e));
                break;
            }
        };
        if let Some(v) = made.decl {
            out.write().push(Outcome::Declared(m.show(v.ty, v.addr, v.count)));
            continue;
        }
        if let Some(shown) = made.assigned {
            out.write().push(Outcome::Assigned(shown));
            continue;
        }
        out.write().push(Outcome::Call { id: made.id.unwrap_or(0), ret: None, how: None, during: None });
        let Ok(news) = JsFuture::from(m.quiet()).await else { return };
        m.snapshot();
        console.write().push_str(&m.console());
        let news: News = serde_wasm_bindgen::from_value(news).unwrap_or(News { finished: Vec::new() });
        for f in news.finished {
            for o in out.write().iter_mut() {
                if let Outcome::Call { id, ret, how, during } = o && *id == f.id {
                    (*ret, *how, *during) = (Some(f.ret), Some(f.how.clone()), Some(j));
                }
            }
        }
    }
}

#[component]
fn App() -> Element {
    let examples = EXAMPLES.with_borrow(|e| e.clone());
    let mut at = use_signal(|| 0usize);
    let out = use_signal(Vec::<Outcome>::new);
    let console = use_signal(String::new);
    let ex = examples[at()].clone();
    // (Each lesson opened: its program made, on a fresh kernel.)
    use_effect(move || {
        let lines = EXAMPLES.with_borrow(|e| e[at()].steps.clone());
        spawn(run(lines, out, console));
    });
    let n = examples.len();
    let in_group: Vec<usize> = (0..n).filter(|&k| examples[k].group == ex.group).collect();
    let place = in_group.iter().position(|&k| k == at()).unwrap_or(0) + 1;
    rsx! {
        header { class: "bar",
            b { "kSEE" }
            select {
                onchange: move |e| at.set(e.value().parse().unwrap_or(0)),
                for (k , x) in examples.iter().enumerate() {
                    option { value: "{k}", selected: k == at(), "{x.group} · {x.title}" }
                }
            }
            button { disabled: at() == 0, onclick: move |_| at -= 1, "◀" }
            button { disabled: at() + 1 >= n, onclick: move |_| at += 1, "▶" }
        }
        main { class: "page",
            article { class: "lesson",
                div { class: "dim", "{ex.group} · {place} of {in_group.len()}" }
                h1 { "{ex.title}" }
                p { class: "question", "{ex.question}" }
                Notebook { ex: ex.clone(), out }
            }
            aside { class: "side",
                h2 { "Console" }
                pre { class: "console", "{console}" }
            }
        }
    }
}

/// The lesson's lines as cells: each line, what it did, and the paragraphs
/// that explain it (those whose first link is to its call); the paragraphs
/// with no link before them all.
#[component]
fn Notebook(ex: Example, out: Signal<Vec<Outcome>>) -> Element {
    let calls: Vec<Option<usize>> = {
        let mut n = 0;
        ex.steps.iter().map(|l| matches!(ksee_core::program::statement(l), Ok(s) if matches!(s.line, ksee_core::program::Line::Call { .. })).then(|| { n += 1; n })).collect()
    };
    let first_call = |p: &str| text::calls(p).first().copied();
    let intro: Vec<String> = ex.text.iter().filter(|p| first_call(p).is_none()).cloned().collect();
    let outs = out.read();
    rsx! {
        for p in intro { text::Paragraph { text: p } }
        ol { class: "cells",
            for (j , line) in ex.steps.iter().enumerate() {
                li { key: "{j}", class: "cell",
                    div { class: "line",
                        code { "{line}" }
                        span { class: "result", {outcome(outs.get(j))} }
                    }
                    for p in ex.text.iter().filter(|p| calls[j].is_some() && first_call(p) == calls[j]) {
                        text::Paragraph { text: p.clone() }
                    }
                }
            }
        }
    }
}

/// An outcome as its line says it.
fn outcome(o: Option<&Outcome>) -> String {
    match o {
        None => "…".into(),
        Some(Outcome::Declared(v)) => format!("= {v}"),
        Some(Outcome::Assigned(v)) => format!("→ {v}"),
        Some(Outcome::Failed(e)) => e.clone(),
        Some(Outcome::Call { ret: None, .. }) => "… blocked".into(),
        Some(Outcome::Call { how: Some(h), .. }) if h == "exited" => "(no return: it exited)".into(),
        Some(Outcome::Call { ret: Some(r), .. }) => format!("= {}", machine().result(*r)),
    }
}
