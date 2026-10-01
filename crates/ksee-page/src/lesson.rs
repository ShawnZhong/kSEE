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
    Call { id: u32, ret: Option<f64>, how: Option<String> },
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
    for line in &lines {
        let made = match m.make(line) {
            Ok(made) => made,
            Err(e) => {
                ui.outcomes.write().push(Outcome::Failed(e));
                break;
            }
        };
        if let Some(v) = made.decl {
            ui.outcomes.write().push(Outcome::Declared(m.show(v.ty, v.addr, v.count)));
            continue;
        }
        if let Some(shown) = made.assigned {
            ui.outcomes.write().push(Outcome::Assigned(shown));
            continue;
        }
        ui.outcomes.write().push(Outcome::Call { id: made.id.unwrap_or(0), ret: None, how: None });
        let Ok(news) = JsFuture::from(m.quiet()).await else { return };
        m.snapshot();
        ui.version += 1;
        ui.console.write().push_str(&m.console());
        let news: News = serde_wasm_bindgen::from_value(news).unwrap_or(News { finished: Vec::new() });
        for f in news.finished {
            for o in ui.outcomes.write().iter_mut() {
                if let Outcome::Call { id, ret, how } = o && *id == f.id {
                    (*ret, *how) = (Some(f.ret), Some(f.how.clone()));
                }
            }
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
/// `N:func` (the cursor at func's first line in it), `:objects` (and the
/// tab that shows what it changed); `try:N`, its program made instead.
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
    if parts.contains(&"objects") {
        ui.tab.set(Tab::Objects);
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
                Cell { key: "{j}", line: line.clone(), outcome: outcomes.get(j).cloned(), call: calls[j] }
                if mine {
                    for p in ex.text.iter().filter(|p| calls[j].is_some() && first(p) == calls[j]) { Paragraph { text: p.clone() } }
                }
            }
        }
    }
}

/// A program's line: what it did, and, a call, its recording selected as
/// it is clicked.
#[component]
fn Cell(line: String, outcome: Option<Outcome>, call: Option<usize>) -> Element {
    let ui = use_context::<Ui>();
    let selected = call.is_some() && call.and_then(|n| call_of(ui, n)) == (ui.call)();
    rsx! {
        li { class: if selected { "cell on" } else { "cell" },
            onclick: move |_| if let Some(k) = call.and_then(|n| call_of(ui, n)) { ui.select(k) },
            code { "{line}" }
            span { class: "result", {said(outcome.as_ref())} }
        }
    }
}

/// An outcome as its line says it.
fn said(o: Option<&Outcome>) -> String {
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
