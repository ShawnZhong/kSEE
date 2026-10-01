//! A lesson: its chapter and place, title and question, and its program as
//! a notebook, each line with what it did and the paragraphs about it;
//! made on a fresh kernel, a line at a time, as it opens.

use dioxus::prelude::*;
use ksee_core::program::{statement, Line};
use serde::Deserialize;

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
    Call { id: u32, ret: Option<i64>, how: Option<String>, during: Option<usize>, changed: Vec<String> },
    Failed(String),
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
        let Ok(finished) = m.quiet().await else { return };
        m.snapshot();
        ui.version += 1;
        ui.console.write().push_str(&m.console());
        for f in finished {
            for (k, o) in ui.outcomes.write().iter_mut().enumerate() {
                if let Outcome::Call { id, ret, how, during, .. } = o && *id == f.id {
                    (*ret, *how, *during) = (Some(f.ret), Some(f.how.to_string()), (k != j).then_some(j));
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
                let records = args.iter().position(|a| a == name).zip(ret.filter(|r| *r > 0))
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

/// The lessons opened before (kept in the browser's storage), by name.
fn seen() -> Vec<String> {
    web_sys::window().and_then(|w| w.local_storage().ok().flatten()).and_then(|s| s.get_item("ksee.seen").ok().flatten())
        .map(|t| t.split(',').map(String::from).collect()).unwrap_or_default()
}

fn see(name: &str) {
    let mut all = seen();
    if !all.iter().any(|n| n == name) {
        all.push(name.into());
        if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
            let _ = s.set_item("ksee.seen", &all.join(","));
        }
    }
}

/// Lesson `k` opened: its own program made, on a fresh kernel.
fn open(mut ui: Ui, k: usize) {
    ui.lesson.set(k);
    ui.program.set(examples()[k].steps.clone());
}

#[component]
pub fn Header() -> Element {
    let ui = use_context::<Ui>();
    let mut menu = use_signal(|| false);
    let all = examples();
    let at = (ui.lesson)();
    let ex = &all[at];
    rsx! {
        header { class: "bar",
            b { title: "watch the Linux kernel run", "kSEE" }
            button { class: if menu() { "on" } else { "" }, title: "every lesson, by chapter", onclick: move |_| menu.toggle(), "☰ Lessons" }
            button { disabled: at == 0, title: "the lesson before", onclick: move |_| open(ui, at - 1), "◀" }
            button { disabled: at + 1 >= all.len(), title: "the next lesson", onclick: move |_| open(ui, at + 1), "▶" }
            span { class: "title", "{ex.group} · " b { "{ex.title}" } }
        }
        if menu() {
            div { class: "menuback", onclick: move |_| menu.set(false) }
            Contents { close: move |_| menu.set(false) }
        }
    }
}

/// Every lesson, by chapter: the one open marked, those opened before ticked.
#[component]
fn Contents(close: EventHandler<()>) -> Element {
    let ui = use_context::<Ui>();
    let all = examples();
    let seen = seen();
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
                        onclick: move |_| { open(ui, k); close.call(()); },
                        span { class: "tick", if seen.contains(&all[k].name) { "✓" } }
                        "{all[k].title}" }
                }
            }
        }
    }
}

/// A line's runner, as written (none: the program's own, "").
fn lane(line: &str) -> String {
    statement(line).ok().and_then(|s| s.runner).unwrap_or_default()
}

#[component]
pub fn Lesson() -> Element {
    let ui = use_context::<Ui>();
    // (The program, made again as it changes: a lesson opened, a try.)
    use_effect(move || {
        let lines = (ui.program)();
        to_url(ui);
        see(&examples()[(ui.lesson)()].name);
        spawn(run(ui, lines));
    });
    let all = examples();
    let at = (ui.lesson)();
    let ex = all[at].clone();
    let group: Vec<usize> = (0..all.len()).filter(|&k| all[k].group == ex.group).collect();
    let place = group.iter().position(|&k| k == at).unwrap_or(0) + 1;
    let program = (ui.program)();
    // (Each line's call number, from 1: the text's links name calls so.)
    let mut n = 0;
    let calls: Vec<Option<usize>> = program.iter().map(|l| matches!(statement(l), Ok(s) if matches!(s.line, Line::Call { .. })).then(|| { n += 1; n })).collect();
    let first = |p: &str| text::calls(p).first().copied();
    let mine = program == ex.steps;
    let outcomes = ui.outcomes.read().clone();
    // (Lanes: a column per runner, with more than one; the program's first.)
    let mut lanes = vec![String::new()];
    for l in &program {
        let r = lane(l);
        if !lanes.contains(&r) {
            lanes.push(r);
        }
    }
    let many = lanes.len() > 1;
    let col = |l: &str| lanes.iter().position(|x| *x == lane(l)).unwrap_or(0) + 1;
    // (A lane waits, at row r, while a call of its asked before r has not returned by then.)
    let waits = |r: usize| -> Vec<usize> {
        program.iter().enumerate().filter(|&(j, _)| j < r).filter_map(|(j, l)| match outcomes.get(j) {
            Some(Outcome::Call { ret, during, .. }) if ret.is_none() || during.is_some_and(|d| d >= r) => Some(col(l)),
            _ => None,
        }).collect()
    };
    let before = at.checked_sub(1).map(|k| (k, all[k].title.clone()));
    let after = (at + 1 < all.len()).then(|| (at + 1, all[at + 1].title.clone()));
    rsx! {
        div { class: "place", "{ex.group} · {place} of {group.len()}" }
        h1 { "{ex.title}" }
        p { class: "question", "{ex.question}" }
        if mine {
            for p in ex.text.iter().filter(|p| first(p).is_none()) { Paragraph { text: p.clone() } }
        } else {
            p { class: "dim", "A try, not the lesson's own program: "
                a { class: "link", onclick: move |_| open(ui, (ui.lesson)()), "↺ back to it" } }
        }
        ol { class: if many { "cells lanes" } else { "cells" }, style: "--lanes: {lanes.len()}",
            if many {
                for (c , l) in lanes.iter().enumerate() {
                    li { class: "lanehead mono", style: "grid-column: {c + 1}", {lane_name(ui, l, &program)} }
                }
            }
            for (j , line) in program.iter().enumerate() {
                Fragment { key: "{j}:{line}",
                    for w in if many { waits(j) } else { Vec::new() } {
                        if w != col(line) { li { class: "wait", style: "grid-column: {w}", title: "a call of this lane's, waiting (blocked)" } }
                    }
                    Cell { j, line: line.clone(), outcome: outcomes.get(j).cloned(), call: calls[j], column: if many { col(line) } else { 0 } }
                    if mine {
                        for p in ex.text.iter().filter(|p| calls[j].is_some() && first(p) == calls[j]) { li { class: "wide", Paragraph { text: p.clone() } } }
                    }
                }
            }
            for (c , l) in lanes.iter().enumerate() {
                {
                    let prefix = if l.is_empty() { String::new() } else { format!("{l}: ") };
                    rsx! {
                        li { class: "cell add", style: if many { format!("grid-column: {}", c + 1) } else { String::new() },
                            crate::editor::Editor { key: "{program.len()}:{c}", text: String::new(), line: program.len(),
                                placeholder: if many { format!("add a call{}…", if l.is_empty() { String::new() } else { format!(", by {l}") }) } else { "add a call…".to_string() },
                                common: common(),
                                done: move |t: String| if !t.is_empty() { let mut ui = ui; ui.program.write().push(format!("{prefix}{t}")); },
                                focus: move |_| {} }
                        }
                    }
                }
            }
        }
        div { class: "ends",
            if let Some((k, t)) = before { a { onclick: move |_| open(ui, k), "← {t}" } } else { span {} }
            if let Some((k, t)) = after { a { onclick: move |_| open(ui, k), b { "Next: {t} →" } } }
        }
    }
}

/// A lane's head: its runner's task, as its first call's syscall has it,
/// else its runner as written.
fn lane_name(ui: Ui, runner: &str, program: &[String]) -> String {
    let id = program.iter().enumerate().filter(|(_, l)| lane(l) == runner).find_map(|(j, _)| match ui.outcomes.read().get(j) {
        Some(Outcome::Call { id, .. }) => Some(*id),
        _ => None,
    });
    let m = machine();
    let task = id.and_then(|id| m.syscall_of(id)).and_then(|k| m.view(|v| v.syscalls().into_iter().nth(k))).map(|s| s.task);
    match task {
        Some(t) => format!("{}({})", t.comm.unwrap_or_default(), t.pid.unwrap_or(0)),
        None if runner.is_empty() => "the program".into(),
        None => runner.into(),
    }
}

/// A program's line `j` (written in place: Enter makes the program again,
/// the line changed, or removed if emptied, or by its ✕): what it did; a
/// call, its recording selected as it gets the focus. In a lane, `column`.
#[component]
fn Cell(j: usize, line: String, outcome: Option<Outcome>, call: Option<usize>, column: usize) -> Element {
    let ui = use_context::<Ui>();
    let selected = call.is_some() && call.and_then(|n| call_of(ui, n)) == (ui.call)();
    // (In a lane, its runner is its column's: not written on the line.)
    let runner = lane(&line);
    let shown = if column > 0 && !runner.is_empty() { statement(&line).map(|s| s.text).unwrap_or(line.clone()) } else { line.clone() };
    let style = if column > 0 { format!("grid-column: {column}") } else { String::new() };
    rsx! {
        li { class: if selected { "cell on" } else { "cell" }, style: "{style}",
            crate::editor::Editor { text: shown, line: j, placeholder: "", common: common(),
                done: move |t: String| {
                    let mut ui = ui;
                    let mut p = ui.program.write();
                    let t = if column > 0 && !runner.is_empty() && !t.is_empty() && lane(&t).is_empty() { format!("{runner}: {t}") } else { t };
                    if t.is_empty() { p.remove(j); } else { p[j] = t; }
                },
                focus: move |_| if let Some(k) = call.and_then(|n| call_of(ui, n)) { ui.select(k) } }
            span { class: "result", {said(outcome.as_ref())} }
            button { class: "x", title: "remove this line", onclick: move |_| { let mut ui = ui; ui.program.write().remove(j); }, "✕" }
        }
        if let Some(Outcome::Call { changed, .. }) = &outcome && !changed.is_empty() {
            li { class: "changed mono", style: "{style}", "{changed.join(\"   \")}" }
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
