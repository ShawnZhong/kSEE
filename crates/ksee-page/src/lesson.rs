//! A lesson: its title and question, the paragraphs
//! that explain it, and its program, each line with what it did (in task
//! columns, with more than one task); made on a fresh kernel, a line
//! at a time, as it opens.

use dioxus::prelude::*;
use ksee_core::program::{statement, Line};
use serde::{Deserialize, Serialize};

use crate::state::{Tab, Ui};
use ksee_core::text::{self, Piece};
use crate::{examples, machine};

/// One chronological program step: a statement for the program's own task,
/// or a statement assigned to the task named by an earlier clone result.
#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(untagged)]
pub enum Step {
    Main(String),
    Task { task: String, text: String },
}

impl Step {
    pub fn text(&self) -> &str {
        match self { Step::Main(text) | Step::Task { text, .. } => text }
    }

    pub fn task(&self) -> Option<&str> {
        match self { Step::Main(_) => None, Step::Task { task, .. } => Some(task) }
    }

    fn with_text(&self, text: String) -> Step {
        match self { Step::Main(_) => Step::Main(text), Step::Task { task, .. } => Step::Task { task: task.clone(), text } }
    }

    fn on(task: Option<&str>, text: String) -> Step {
        match task { Some(task) => Step::Task { task: task.into(), text }, None => Step::Main(text) }
    }
}

/// A lesson (examples.json): its chapter, title, question, explanation,
/// tries, and its program's steps.
#[derive(Deserialize, Clone, PartialEq)]
pub struct Example {
    pub name: String,
    pub group: String,
    pub title: String,
    pub question: String,
    pub text: Vec<String>,
    #[serde(default)]
    pub tries: Vec<Vec<Step>>,
    pub steps: Vec<Step>,
}

/// How a line went: a variable declared (as it starts), memory written
/// (what it holds then), or a call (its id): finished (its result) or not
/// yet (it blocks); or why it could not be made.
#[derive(Clone, PartialEq)]
pub enum Outcome {
    Declared(String),
    /// (`during`: the line during which it finished, if a later one's;
    /// `changed`: the variables it changed, as they are then.)
    Call { id: u32, ret: Option<i64>, how: Option<String>, during: Option<usize>, changed: Vec<String> },
    Failed(String),
}


/// Make `lines` on a fresh kernel, one after another (a call: then the
/// kernel run until it is quiet), each line's outcome as it goes; then a
/// call selected: the first the lesson's text links to (its subject), else
/// the last.
async fn run(mut ui: Ui, lines: Vec<Step>) {
    let m = machine();
    ui.outcomes.write().clear();
    ui.at.set(None);
    ui.peek.set(None);
    if m.restart().await.is_err() {
        return;
    }
    ui.console.set(String::new());
    // (The variables so far: their name, type, address, count, and as they were shown last.)
    let mut vars: Vec<(String, u32, u32, Option<u32>, u32, String)> = Vec::new();
    for (j, step) in lines.iter().enumerate() {
        let line = step.text();
        let made = match m.make(step.task(), line) {
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
        let Some(id) = made.id else { continue };
        ui.outcomes.write().push(Outcome::Call { id, ret: None, how: None, during: None, changed: Vec::new() });
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
        let (call, args) = match statement(line) {
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
    // (The lesson's own program: where its text's first link into the recording goes.)
    let linked = (lines == ex.steps).then(|| ex.text.iter().flat_map(|p| text::pieces(p)).find_map(|x| match x {
        Piece::Link { call: Some(_), target, .. } => Some(target),
        _ => None,
    })).flatten();
    match linked {
        Some(t) => follow(ui, &t),
        None => if let Some(k) = call_of(ui, calls) { ui.select(k) },
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
            ui.program.set(t.clone());
        }
        return;
    }
    let Some(k) = parts[0].parse().ok().and_then(|n| call_of(ui, n)) else { return };
    ui.select(k);
    if let Some(func) = parts.get(1) {
        // (The call's lines and syscall now: the memos have them as they were.)
        let m = machine();
        let (lines, s) = (Ui::lines_of(k), m.view(|v| v.syscalls().into_iter().nth(k)));
        let at = s.and_then(|s| m.first_call(func, s.first, s.last.unwrap_or(m.records() - 1), (!parts.contains(&"any")).then_some(s.task.addr)));
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

/// The URL's state: #ex=NAME (the lesson), &p=JSON (its program, if
/// changed).
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
fn program_json(program: &[Step]) -> Option<String> {
    let value = serde_wasm_bindgen::to_value(program).ok()?;
    js_sys::JSON::stringify(&value).ok()?.as_string()
}

fn program_from_json(text: &str) -> Option<Vec<Step>> {
    serde_wasm_bindgen::from_value(js_sys::JSON::parse(text).ok()?).ok()
}

pub fn from_url(mut ui: Ui) {
    let (ex, p) = hash();
    let all = examples();
    if let Some(k) = ex.and_then(|n| all.iter().position(|e| e.name == n)) {
        ui.lesson.set(k);
        ui.program.set(p.as_deref().and_then(program_from_json).unwrap_or_else(|| all[k].steps.clone()));
    }
}

/// The URL, as the lesson and its program are.
fn to_url(ui: Ui) {
    let ex = &examples()[(ui.lesson)()];
    let program = (ui.program)();
    let p = if program == ex.steps { String::new() } else {
        program_json(&program).map_or_else(String::new, |p| format!("&p={}", js_sys::encode_uri_component(&p)))
    };
    if let Some(h) = web_sys::window().and_then(|w| w.history().ok()) {
        let _ = h.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&format!("#ex={}{p}", ex.name)));
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
    // (Where the lesson is: its chapter, and which of its lessons.)
    let group: Vec<usize> = (0..all.len()).filter(|&k| all[k].group == all[at].group).collect();
    let place = group.iter().position(|&k| k == at).unwrap_or(0) + 1;
    rsx! {
        header { class: "topbar",
            b { title: "watch the Linux kernel run", "kSEE" }
            button { class: if menu() { "contents on" } else { "contents" }, title: "every lesson, by chapter", onclick: move |_| menu.toggle(), "☰ Lessons" }
            button { disabled: at == 0, title: "the lesson before", onclick: move |_| open(ui, at - 1), "◀" }
            button { disabled: at + 1 >= all.len(), title: "the next lesson", onclick: move |_| open(ui, at + 1), "▶" }
            span { class: "crumb", "{all[at].group} · {place} of {group.len()}" }
            span { class: "keys dim", title: "← → a line (Shift: over the calls)\n↑ ↓ the frame, the callee or the caller\nhover the timeline: that moment; click: go there\ndrag: move the timeline; wheel: move left/right, zoom up/down\nCtrl+click a name: its definition", "keys" }
            Theme {}
            if menu() {
                div { class: "menuback", onclick: move |_| menu.set(false) }
                Contents { close: move |_| menu.set(false) }
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
                        onclick: move |_| { open(ui, k); close.call(()); },
                        "{all[k].title}" }
                }
            }
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
        // (Once the kernel is up.)
        if matches!((ui.kernel)(), Some(Ok(()))) {
            spawn(run(ui, lines));
        }
    });
    let all = examples();
    let at = (ui.lesson)();
    let ex = all[at].clone();
    let program = (ui.program)();
    // (Each step's call number, from 1: the text's links name calls so.)
    let mut n = 0;
    let calls: Vec<Option<usize>> = program.iter().map(|step| matches!(statement(step.text()), Ok(Line::Call { .. })).then(|| { n += 1; n })).collect();
    let mine = program == ex.steps;
    let outcomes = ui.outcomes.read().clone();
    // (Task columns: the program's first, then those used or made.)
    let active = if matches!((ui.kernel)(), Some(Ok(()))) { machine().task_columns(program.len()) } else { Vec::new() };
    let mut columns: Vec<Option<String>> = vec![None];
    for task in program.iter().filter_map(|step| step.task()).chain(active.iter().map(String::as_str)) {
        if !columns.iter().any(|column| column.as_deref() == Some(task)) {
            columns.push(Some(task.into()));
        }
    }
    let many = columns.len() > 1;
    let col = |step: &Step| columns.iter().position(|column| column.as_deref() == step.task()).unwrap_or(0);
    let names: Vec<(String, i32)> = columns.iter().map(|task| task_name(ui, task.as_deref(), &program)).collect();
    // (A task waits at row r while one of its earlier calls has not returned.)
    let waits = |r: usize| -> Vec<usize> {
        let mut waiting: Vec<usize> = program.iter().enumerate().filter(|&(j, _)| j < r).filter_map(|(j, step)| match outcomes.get(j) {
            Some(Outcome::Call { ret, during, .. }) if ret.is_none() || during.is_some_and(|d| d >= r) => Some(col(step)),
            _ => None,
        }).collect();
        waiting.dedup();
        waiting
    };
    let grid = format!("--columns: {}", columns.len());
    rsx! {
        h1 { "{ex.title}" }
        p { class: "question", "{ex.question}" }
        if mine {
            div { class: "explain", for p in ex.text.iter() { Paragraph { text: p.clone(), stale: false } } }
        } else {
            // (A try: its own text kept, the lesson's still shown; its call
            // links no longer apply.)
            div { class: "explain", for p in ex.text.iter() { Paragraph { text: p.clone(), stale: true } } }
            p { class: "explain dim", "A try, not the lesson's own program." }
        }
        div { class: "ptitle dim", "Program"
            if !mine { a { title: "the lesson's own program again, on a fresh kernel", onclick: move |_| open(ui, (ui.lesson)()), "↺ reset" } }
        }
        div { class: "program",
            if many {
                div { class: "taskheads", style: "{grid}",
                    for (name , pid) in names.iter().cloned() { div { class: "task mono", style: "--hue: {crate::chart::hue(pid)}", "{name}" } }
                }
            }
            ol { class: if many { "steps columns" } else { "steps" }, style: "{grid}",
                for (j , step) in program.iter().enumerate() {
                    li { key: "{j}:{step.text()}", class: if many { "row" } else { "" },
                        if many {
                            for w in waits(j).into_iter().filter(|&w| w != col(step)) {
                                div { class: "wait", style: "grid-column: {w + 1}; --hue: {crate::chart::hue(names[w].1)}", title: "a call in this task column, waiting (blocked)" }
                            }
                        }
                        Cell { j, step: step.clone(), outcome: outcomes.get(j).cloned(), call: calls[j],
                            column: if many { Some((col(step), crate::chart::hue(names[col(step)].1))) } else { None } }
                    }
                }
                li { class: if many { "add row" } else { "add" },
                    for (c , task) in columns.iter().enumerate() {
                        {
                            let task = task.clone();
                            let by = if many { format!("add a call, by {}…", names[c].0) } else { "add a call…".to_string() };
                            rsx! {
                                div { class: "cell", style: if many { format!("grid-column: {}", c + 1) } else { String::new() },
                                    crate::editor::Editor { key: "{program.len()}:{c}", text: String::new(), task: task.clone(), line: program.len(),
                                        placeholder: by, common: common(),
                                        done: move |text: String| if !text.is_empty() { let mut ui = ui; ui.program.write().push(Step::on(task.as_deref(), text)); },
                                        focus: move |_| {} }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// A task column's head: its task's recorded name and pid, or the name the
/// program gave it before its first call runs.
fn task_name(ui: Ui, task: Option<&str>, program: &[Step]) -> (String, i32) {
    let id = program.iter().enumerate().filter(|(_, step)| step.task() == task).find_map(|(j, _)| match ui.outcomes.read().get(j) {
        Some(Outcome::Call { id, .. }) => Some(*id),
        _ => None,
    });
    // (A call made: the kernel is up.)
    let recorded = id.and_then(|id| {
        let m = machine();
        m.syscall_of(id).and_then(|k| m.view(|v| v.syscalls().into_iter().nth(k))).map(|s| s.task)
    });
    match recorded {
        Some(t) => (t.named(), t.pid.unwrap_or(0)),
        None if task.is_none() => ("the program".into(), 0),
        None => (task.unwrap_or_default().into(), 0),
    }
}

/// A program's step `j` (written in place: Enter makes the program again,
/// the text changed, or removed if emptied, or by its ✕): what it did; a
/// call, its recording selected as it gets the focus. In a task column: its
/// column, and its task's hue.
#[component]
fn Cell(j: usize, step: Step, outcome: Option<Outcome>, call: Option<usize>, column: Option<(usize, i32)>) -> Element {
    let ui = use_context::<Ui>();
    let selected = call.is_some() && call.and_then(|n| call_of(ui, n)) == ui.call();
    let style = column.as_ref().map_or(String::new(), |(c, h)| format!("grid-column: {}; --hue: {h}", c + 1));
    let blocked = matches!(outcome, Some(Outcome::Call { ret: None, .. }));
    let class = format!("cell{}{}", if selected { " on" } else { "" }, if blocked { " blocked" } else { "" });
    rsx! {
        div { class, style: "{style}",
            div { class: "line",
                crate::editor::Editor { text: step.text().to_string(), task: step.task().map(String::from), line: j, placeholder: "", common: common(),
                    done: move |text: String| {
                        let mut ui = ui;
                        let mut program = ui.program.write();
                        if text.is_empty() { program.remove(j); } else { program[j] = step.with_text(text); }
                    },
                    focus: move |_| if let Some(k) = call.and_then(|n| call_of(ui, n)) { ui.select(k) } }
                a { class: "rm", title: "remove this line (the program runs again without it)", onclick: move |_| { let mut ui = ui; ui.program.write().remove(j); }, "✕" }
                span { class: "ret mono", {said(outcome.as_ref())} }
            }
            if let Some(Outcome::Call { changed, .. }) = &outcome && !changed.is_empty() {
                div { class: "more mono dim", "{changed.join(\"   \")}" }
            }
        }
    }
}

/// The syscalls the lessons make: listed first.
fn common() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for step in examples().iter().flat_map(|e| e.steps.iter()) {
        if let Ok(Line::Call { call, .. }) = statement(step.text()) && !out.contains(&call) {
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
fn Paragraph(text: String, stale: bool) -> Element {
    let ui = use_context::<Ui>();
    rsx! {
        p { class: "para",
            for x in text::pieces(&text) {
                match x {
                    Piece::Text(t) => rsx! { "{t}" },
                    Piece::Code(c) => rsx! { code { "{c}" } },
                    Piece::Link { label, target, call } => {
                        // (A try: a button; a function's place: its name, as code.)
                        let class = if target.starts_with("try:") { "try" } else if target.contains(':') { "place mono" } else { "" };
                        // (Stale (a try, not the lesson's own program): a call
                        // link names a call it no longer has: dimmed, no link.)
                        if stale && call.is_some() {
                            rsx! { span { class: "dim", "{label}" } }
                        } else {
                            rsx! { a { class, onclick: move |_| follow(ui, &target), "{label}" } }
                        }
                    }
                }
            }
        }
    }
}
