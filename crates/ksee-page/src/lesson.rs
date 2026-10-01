//! A lesson: its chapter and place, title and question, and its program as
//! a notebook, each line with what it did and the paragraphs about it;
//! made on a fresh kernel, a line at a time, as it opens.

use dioxus::prelude::*;
use ksee_core::program::{statement, Line};
use serde::Deserialize;
use wasm_bindgen_futures::JsFuture;

use crate::state::{Tab, Ui};
use crate::text::{self, Piece};
use crate::{examples, machine};

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

/// How a line went: a variable declared (as it starts), memory written
/// (what it holds then), or a call (its id): finished (its result) or not
/// yet (it blocks); or why it could not be made.
#[derive(Clone, PartialEq)]
pub enum Outcome {
    Declared(String),
    Assigned(String),
    /// (`during`: the line during which it finished, if a later one's;
    /// `changed`: the variables it changed, as they are then.)
    Call { id: u32, ret: Option<f64>, how: Option<String>, during: Option<usize>, changed: Vec<String> },
    Failed(String),
}

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

/// Make `lines` on a fresh kernel, one after another (a call: then the
/// kernel run until it is quiet), each line's outcome as it goes; then a
/// call selected: the first the lesson's text links to (its subject), else
/// the last.
async fn run(mut ui: Ui, lines: Vec<String>) {
    let m = machine();
    ui.outcomes.write().clear();
    ui.call.set(None);
    if m.restart().await.is_err() {
        return;
    }
    ui.console.set(String::new());
    // (The variables so far: their name, type, address, count, and as they were shown last.)
    let mut vars: Vec<(String, u32, u32, Option<u32>, u32, String)> = Vec::new();
    for (j, line) in lines.iter().enumerate() {
        let made = match m.make(line) {
            Ok(made) => made,
            Err(e) => {
                ui.outcomes.write().push(Outcome::Failed(e));
                break;
            }
        };
        if let Some(v) = made.decl {
            let shown = m.show(v.ty, v.addr, v.count);
            vars.push((v.name, v.ty, v.addr, v.count, v.size * v.count.unwrap_or(1), shown.clone()));
            ui.outcomes.write().push(Outcome::Declared(shown));
            continue;
        }
        if let Some(shown) = made.assigned {
            ui.outcomes.write().push(Outcome::Assigned(shown));
            continue;
        }
        ui.outcomes.write().push(Outcome::Call { id: made.id.unwrap_or(0), ret: None, how: None, during: None, changed: Vec::new() });
        let Ok(news) = JsFuture::from(m.quiet()).await else { return };
        m.snapshot();
        ui.version += 1;
        ui.console.write().push_str(&m.console());
        let news: News = serde_wasm_bindgen::from_value(news).unwrap_or(News { finished: Vec::new() });
        for f in news.finished {
            for (k, o) in ui.outcomes.write().iter_mut().enumerate() {
                if let Outcome::Call { id, ret, how, during, .. } = o && *id == f.id {
                    (*ret, *how, *during) = (Some(f.ret), Some(f.how.clone()), (k != j).then_some(j));
                }
            }
        }
        // (The variables the line's call changed, by now; one passed as records of a
        // variable length, as its parameter's type reads them: a directory's entries.)
        let (call, args) = match statement(line).map(|s| s.line) {
            Ok(Line::Call { call, texts, .. }) => (call, texts),
            _ => (String::new(), Vec::new()),
        };
        let ret = match ui.outcomes.read().get(j) { Some(Outcome::Call { ret, .. }) => *ret, _ => None };
        let mut changed = Vec::new();
        for (name, ty, addr, count, size, last) in vars.iter_mut() {
            let now = m.show(*ty, *addr, *count);
            if now != *last {
                let records = args.iter().position(|a| a == name).zip(ret.filter(|r| *r > 0.0))
                    .and_then(|(k, r)| m.as_records(&call, k, *addr, (r as u32).min(*size)));
                changed.push(format!("{name} = {}", records.unwrap_or_else(|| now.clone())));
                *last = now;
            }
        }
        if let Some(Outcome::Call { changed: c, .. }) = ui.outcomes.write().get_mut(j) {
            *c = changed;
        }
    }
    let ex = &examples()[(ui.lesson)()];
    let calls = ui.outcomes.read().iter().filter(|o| matches!(o, Outcome::Call { .. })).count();
    let linked = (lines == ex.steps).then(|| ex.text.iter().find_map(|p| text::calls(p).first().copied())).flatten();
    if let Some(k) = call_of(ui, linked.unwrap_or(calls)) {
        ui.select(k);
    }
}

/// The syscall (an index in the syscalls) the program's call `n` (from 1)
/// made, if the recording has it.
fn call_of(ui: Ui, n: usize) -> Option<usize> {
    let id = ui.outcomes.read().iter().filter_map(|o| match o {
        Outcome::Call { id, .. } => Some(*id),
        _ => None,
    }).nth(n.checked_sub(1)?)?;
    machine().syscall_of(id)
}

/// Follow link `target` of the lesson's text: `N` (its call N selected),
/// `N:func` (the cursor at func's first line in it), and a tab to show
/// (`:stack`, `:objects`, `:tasks`, `:console`); `try:N`, its program made
/// instead.
fn follow(mut ui: Ui, target: &str) {
    let parts: Vec<&str> = target.split(':').collect();
    if parts[0] == "try" {
        let k: usize = parts.get(1).and_then(|n| n.parse().ok()).unwrap_or(1);
        let tries = examples()[(ui.lesson)()].tries.clone();
        if let Some(t) = tries.get(k - 1) {
            ui.program.set(machine().program_lines(t));
        }
        return;
    }
    let Some(k) = parts[0].parse().ok().and_then(|n| call_of(ui, n)) else { return };
    ui.select(k);
    if let Some(func) = parts.get(1) {
        let (lines, s) = (ui.lines(), ui.syscall());
        let m = machine();
        let at = s.and_then(|s| m.first_call(func, s.first, s.last.unwrap_or(m.records() - 1), Some(s.task.addr)));
        if let Some(at) = at && let Some(j) = lines.iter().position(|l| l.i >= at) {
            ui.go(j);
        }
    }
    for (name, tab) in [("stack", Tab::Stack), ("objects", Tab::Objects), ("tasks", Tab::Tasks), ("console", Tab::Console)] {
        if parts.contains(&name) {
            ui.tab.set(tab);
        }
    }
}

/// The URL's state: #ex=NAME (the lesson), &p=LINES (its program, if
/// changed: its lines separated by `;`).
fn hash() -> (Option<String>, Option<String>) {
    let h = web_sys::window().and_then(|w| w.location().hash().ok()).unwrap_or_default();
    let mut ex = None;
    let mut p = None;
    for kv in h.trim_start_matches('#').split('&') {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        let v = js_sys::decode_uri_component(v).ok().and_then(|v| v.as_string()).unwrap_or_default();
        match k {
            "ex" => ex = Some(v),
            "p" => p = Some(v),
            _ => {}
        }
    }
    (ex, p)
}

/// The lesson and program the URL says, at first.
pub fn from_url(mut ui: Ui) {
    let (ex, p) = hash();
    let all = examples();
    if let Some(k) = ex.and_then(|n| all.iter().position(|e| e.name == n)) {
        ui.lesson.set(k);
        ui.program.set(p.map_or_else(|| all[k].steps.clone(), |p| machine().program_lines(&p)));
    }
}

/// The URL, as the lesson and its program are.
fn to_url(ui: Ui) {
    let ex = &examples()[(ui.lesson)()];
    let program = (ui.program)();
    let p = if program == ex.steps { String::new() } else { format!("&p={}", js_sys::encode_uri_component(&program.join("; "))) };
    if let Some(h) = web_sys::window().and_then(|w| w.history().ok()) {
        let _ = h.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&format!("#ex={}{p}", ex.name)));
    }
}

#[component]
pub fn Header() -> Element {
    let mut ui = use_context::<Ui>();
    let all = examples();
    let n = all.len();
    let mut open = move |k: usize| {
        ui.lesson.set(k);
        ui.program.set(examples()[k].steps.clone());
    };
    let at = (ui.lesson)();
    rsx! {
        header { class: "bar",
            b { "kSEE" }
            select {
                onchange: move |e| open(e.value().parse().unwrap_or(0)),
                for (k , x) in all.iter().enumerate() {
                    option { value: "{k}", selected: k == at, "{x.group} · {x.title}" }
                }
            }
            button { disabled: at == 0, onclick: move |_| open(at - 1), "◀" }
            button { disabled: at + 1 >= n, onclick: move |_| open(at + 1), "▶" }
        }
    }
}

#[component]
pub fn Lesson() -> Element {
    let ui = use_context::<Ui>();
    // (The program, made again as it changes: a lesson opened, a try.)
    use_effect(move || {
        let lines = (ui.program)();
        to_url(ui);
        spawn(run(ui, lines));
    });
    let all = examples();
    let ex = all[(ui.lesson)()].clone();
    let group: Vec<usize> = (0..all.len()).filter(|&k| all[k].group == ex.group).collect();
    let place = group.iter().position(|&k| k == (ui.lesson)()).unwrap_or(0) + 1;
    let program = (ui.program)();
    // (Each line's call number, from 1: the text's links name calls so.)
    let mut n = 0;
    let calls: Vec<Option<usize>> = program.iter().map(|l| matches!(statement(l), Ok(s) if matches!(s.line, Line::Call { .. })).then(|| { n += 1; n })).collect();
    let first = |p: &str| text::calls(p).first().copied();
    let mine = program == ex.steps;
    let outcomes = ui.outcomes.read().clone();
    rsx! {
        div { class: "dim", "{ex.group} · {place} of {group.len()}" }
        h1 { "{ex.title}" }
        p { class: "question", "{ex.question}" }
        if mine {
            for p in ex.text.iter().filter(|p| first(p).is_none()) { Paragraph { text: p.clone() } }
        } else {
            p { class: "dim", "A try, not the lesson's own program: "
                a { class: "link", onclick: move |_| { let mut ui = ui; ui.program.set(examples()[(ui.lesson)()].steps.clone()); }, "back to it" } }
        }
        ol { class: "cells",
            for (j , line) in program.iter().enumerate() {
                Cell { key: "{j}:{line}", j, line: line.clone(), outcome: outcomes.get(j).cloned(), call: calls[j] }
                if mine {
                    for p in ex.text.iter().filter(|p| calls[j].is_some() && first(p) == calls[j]) { Paragraph { text: p.clone() } }
                }
            }
            li { class: "cell add",
                crate::editor::Editor { key: "{program.len()}", text: String::new(), line: program.len(), placeholder: "add a call…", common: common(),
                    done: move |t: String| if !t.is_empty() { let mut ui = ui; ui.program.write().push(t); },
                    focus: move |_| {} }
            }
        }
    }
}

/// A program's line `j` (written in place: Enter makes the program again,
/// the line changed, or removed if emptied): what it did; a call, its
/// recording selected as it gets the focus.
#[component]
fn Cell(j: usize, line: String, outcome: Option<Outcome>, call: Option<usize>) -> Element {
    let ui = use_context::<Ui>();
    let selected = call.is_some() && call.and_then(|n| call_of(ui, n)) == (ui.call)();
    rsx! {
        li { class: if selected { "cell on" } else { "cell" },
            crate::editor::Editor { text: line.clone(), line: j, placeholder: "", common: common(),
                done: move |t: String| {
                    let mut ui = ui;
                    let mut p = ui.program.write();
                    if t.is_empty() { p.remove(j); } else { p[j] = t; }
                },
                focus: move |_| if let Some(k) = call.and_then(|n| call_of(ui, n)) { ui.select(k) } }
            span { class: "result", {said(outcome.as_ref())} }
        }
        if let Some(Outcome::Call { changed, .. }) = &outcome && !changed.is_empty() {
            div { class: "changed mono dim", "{changed.join(\"   \")}" }
        }
    }
}

/// The syscalls the lessons make: listed first.
fn common() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for l in examples().iter().flat_map(|e| e.steps.iter()) {
        if let Ok(s) = statement(l) && let Line::Call { call, .. } = s.line && !out.contains(&call) {
            out.push(call);
        }
    }
    out
}

/// An outcome as its line says it.
fn said(o: Option<&Outcome>) -> String {
    match o {
        None => "…".into(),
        Some(Outcome::Declared(v)) => format!("= {v}"),
        Some(Outcome::Assigned(v)) => format!("→ {v}"),
        Some(Outcome::Failed(e)) => e.clone(),
        Some(Outcome::Call { ret: None, .. }) => "… blocked".into(),
        Some(Outcome::Call { how: Some(h), during, .. }) if h != "returned" => format!("(no return: {}{})", if h == "exec" { "it ran a program" } else { "it exited" }, later(*during)),
        Some(Outcome::Call { ret: Some(r), during, .. }) => format!("= {}{}", machine().result(*r), later(*during)),
    }
}

/// " (during line N)", for a call that finished as a later line ran.
fn later(during: Option<usize>) -> String {
    during.map_or(String::new(), |j| format!(" (during line {})", j + 1))
}

#[component]
fn Paragraph(text: String) -> Element {
    let ui = use_context::<Ui>();
    rsx! {
        p { class: "para",
            for x in text::pieces(&text) {
                match x {
                    Piece::Text(t) => rsx! { "{t}" },
                    Piece::Code(c) => rsx! { code { "{c}" } },
                    Piece::Link { label, target, .. } => rsx! { a { class: "link", onclick: move |_| follow(ui, &target), "{label}" } },
                }
            }
        }
    }
}
