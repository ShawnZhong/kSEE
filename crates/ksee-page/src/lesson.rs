//! A lesson: its title and question, the paragraphs
//! that explain it, and its program, each line with what it did (in task
//! columns, with more than one task); made on a fresh kernel, a line
//! at a time, as it opens.

use dioxus::prelude::*;
use ksee_core::program::{statement, Line};
use ksee_core::text::{self, LessonTarget, Piece};
use ksee_host::machine::Made;
use ksee_host::Outcome;
use serde::{Deserialize, Serialize};

use crate::state::Ui;
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

/// Make `lines` on a fresh kernel, one after another (a call: then the
/// kernel run until it is quiet), each line's outcome (the Machine's) shown
/// as it goes; then, the timeline the whole run, a
/// call selected: the first the lesson's text links to (its subject), else
/// the last.
/// (A run begun again cancels this one: it stops at its next await.)
pub(crate) async fn run(mut ui: Ui, lines: Vec<Step>) {
    let m = machine();
    if m.restart().await.is_err() {
        return;
    }
    for step in &lines {
        let made = m.make(step.task(), step.text());
        ui.outcomes.set(m.outcomes());
        match made {
            Err(_) => break,
            Ok(Made::Decl(_)) => continue,
            Ok(Made::Call(_)) => {}
        }
        if m.quiet().await.is_err() {
            return;
        }
        m.snapshot();
        ui.outcomes.set(m.outcomes());
        ui.console.write().push_str(&m.console());
        ui.snapshot += 1;
    }
    let outcomes = m.outcomes();
    let ex = &examples()[(ui.lesson)()];
    let calls = outcomes.iter().filter(|o| matches!(o, Outcome::Call { .. })).count();
    // (The lesson's own program: where its text's first link into the recording goes.)
    let linked = (lines == ex.steps).then(|| ex.text.iter().flat_map(|p| text::pieces(p)).find_map(|x| match x {
        Piece::Link { to: Some(to @ LessonTarget::Recording { .. }), .. } => Some(to),
        _ => None,
    })).flatten();
    match linked {
        Some(to) => follow(ui, to),
        None => if let Some(k) = call_of(ui, calls) { ui.select(k) },
    }
    // (The timeline: the whole run, the program's calls each in place; the
    // cursor in its subject.)
    ui.win.set(None);
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
fn follow(mut ui: Ui, target: LessonTarget) {
    let LessonTarget::Recording { call, function, any_task, tab } = target else {
        let LessonTarget::Try { index } = target else { unreachable!() };
        if let Some(t) = examples()[(ui.lesson)()].tries.get(index - 1) {
            ui.program.set(t.clone());
        }
        return;
    };
    let Some(k) = call_of(ui, call) else { return };
    ui.select(k);
    if let Some(func) = function {
        // (The call's lines and syscall now: the memos have them as they were.)
        let m = machine();
        let (lines, s) = (Ui::lines_of(k), m.view(|v| v.syscall(k)));
        let at = s.and_then(|s| m.first_call(&func, s.first, s.last, (!any_task).then_some(s.task.addr)));
        if let Some(at) = at && let Some(j) = lines.iter().position(|l| l.i >= at) {
            ui.go(j);
        }
    }
    if let Some(tab) = tab {
        ui.tab.set(tab);
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
    let (ex, p) = hash();
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
        program_json(program).map_or_else(String::new, |p| format!("&p={}", js_sys::encode_uri_component(&p)))
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
            b { title: "watch the Linux kernel run", "kSEE" }
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
            if let Some(e) = at.checked_sub(1).map(|k| &all[k]) {
                a { title: "the lesson before", onclick: move |_| ui.open_lesson(at - 1), "◀ {e.title}" }
            }
            if let Some(e) = all.get(at + 1) {
                a { class: "next", title: "the next lesson", onclick: move |_| ui.open_lesson(at + 1), "{e.title} ▶" }
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

#[component]
pub fn Lesson() -> Element {
    let mut ui = use_context::<Ui>();
    let all = examples();
    let at = (ui.lesson)();
    let ex = &all[at];
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
            Some(Outcome::Call { done, during, .. }) if !done || during.is_some_and(|d| d >= r) => Some(col(step)),
            _ => None,
        }).collect();
        waiting.dedup();
        waiting
    };
    let grid = format!("--columns: {}", columns.len());
    rsx! {
        h1 { "{ex.title}" }
        p { class: "question", "{ex.question}" }
        // (A try: the lesson's text still shown, its call links no longer apply.)
        div { class: "explain", for p in ex.text.iter() {
            ParagraphView { pieces: text::pieces(p), stale: !mine, follow: move |target| follow(ui, target) }
        } }
        if !mine { p { class: "explain dim", "A try, not the lesson's own program." } }
        div { class: "ptitle dim", "Program"
            if !mine { a { title: "the lesson's own program again, on a fresh kernel", onclick: move |_| ui.open_lesson(at), "↺ reset" } }
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
                                        placeholder: by,
                                        done: move |text: String| if !text.is_empty() { ui.program.write().push(Step::on(task.as_deref(), text)); },
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
        m.syscall_of(id).and_then(|k| m.view(|v| v.syscall(k))).map(|s| s.task)
    });
    match recorded {
        Some(t) => (t.named(), t.pid.unwrap_or(0)),
        None if task.is_none() => ("the program".into(), 0),
        None => (task.unwrap_or_default().into(), 0),
    }
}

/// A program's step `j` (written in place: Enter makes the program again,
/// the text changed, or removed if emptied): what it did; a
/// call, its recording selected as it gets the focus. In a task column: its
/// column, and its task's hue.
#[component]
fn Cell(j: usize, step: Step, outcome: Option<Outcome>, call: Option<usize>, column: Option<(usize, i32)>) -> Element {
    let mut ui = use_context::<Ui>();
    let selected = call.is_some() && call.and_then(|n| call_of(ui, n)) == ui.call();
    let style = column.as_ref().map_or(String::new(), |(c, h)| format!("grid-column: {}; --hue: {h}", c + 1));
    let blocked = matches!(outcome, Some(Outcome::Call { done: false, .. }));
    let class = format!("cell{}{}", if selected { " on" } else { "" }, if blocked { " blocked" } else { "" });
    rsx! {
        div { class, style: "{style}",
            div { class: "line",
                crate::editor::Editor { text: step.text().to_string(), task: step.task().map(String::from), line: j, placeholder: "",
                    done: move |text: String| {
                        let mut program = ui.program.write();
                        if text.is_empty() { program.remove(j); } else { program[j] = step.with_text(text); }
                    },
                    focus: move |_| if let Some(k) = call.and_then(|n| call_of(ui, n)) { ui.select(k) } }
                span { class: "ret mono", {said(outcome.as_ref())} }
            }
            if let Some(Outcome::Call { changed, .. }) = &outcome && !changed.is_empty() {
                div { class: "more mono dim", "{changed.join(\"   \")}" }
            }
        }
    }
}

thread_local! {
    /// The syscalls the lessons make: listed first (found once, as the
    /// first editor asks).
    static COMMON: &'static [String] = {
        let mut out: Vec<String> = Vec::new();
        for step in examples().iter().flat_map(|e| e.steps.iter()) {
            if let Ok(Line::Call { call, .. }) = statement(step.text()) && !out.contains(&call) {
                out.push(call);
            }
        }
        Box::leak(out.into_boxed_slice())
    };
}

pub fn common() -> &'static [String] {
    COMMON.with(|c| *c)
}

/// An outcome as its line says it.
fn said(o: Option<&Outcome>) -> String {
    match o {
        None => "…".into(),
        Some(Outcome::Declared(v)) => format!("= {v}"),
        Some(Outcome::Failed(e)) => e.clone(),
        Some(Outcome::Call { done: false, .. }) => "… blocked".into(),
        Some(Outcome::Call { ret: None, during, .. }) => format!("(no return{})", later(*during)),
        Some(Outcome::Call { ret: Some(r), during, .. }) => format!("= {}{}", machine().tables().types.result(*r), later(*during)),
    }
}

/// " (during line N)", for a call that finished as a later line ran.
fn later(during: Option<usize>) -> String {
    during.map_or(String::new(), |j| format!(" (during line {})", j + 1))
}

/// A parsed lesson paragraph. Recording links are disabled when the lesson's
/// program is stale; try links remain available.
#[component]
pub(crate) fn ParagraphView(
    pieces: Vec<Piece>,
    stale: bool,
    follow: EventHandler<LessonTarget>,
) -> Element {
    rsx! {
        p {
            for piece in pieces {
                match piece {
                    Piece::Text(text) => rsx! { "{text}" },
                    Piece::Code(code) => rsx! { code { "{code}" } },
                    Piece::Link { label, to, .. } => match to {
                        Some(to) if !(stale && matches!(to, LessonTarget::Recording { .. })) => {
                            let class = match &to {
                                LessonTarget::Try { .. } => "try",
                                LessonTarget::Recording { function: Some(_), .. } => "place mono",
                                _ => "",
                            };
                            rsx! { a { class, onclick: move |_| follow.call(to.clone()), "{label}" } }
                        }
                        _ => rsx! { span { class: "dim", "{label}" } },
                    },
                }
            }
        }
    }
}
