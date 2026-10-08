//! A lesson: its title, the paragraphs
//! that explain it, and its program, each line with what it did (in task
//! columns, with more than one task); made on a fresh kernel, a line
//! at a time, as it opens.

use dioxus::prelude::*;
use ksee_core::program::{statement, Statement};
use ksee_core::text::{self, LessonTarget, Piece};
use ksee_host::machine::Made;
use ksee_host::Outcome;
use serde::{Deserialize, Serialize};

use crate::state::Ui;
use crate::{lessons, machine};

/// A line of a program, in order: a statement for the program's own task,
/// or one for the task named by an earlier clone's result.
#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(untagged)]
pub enum ProgramLine {
    Main(String),
    Task { task: String, text: String },
}

impl ProgramLine {
    pub fn text(&self) -> &str {
        match self { ProgramLine::Main(text) | ProgramLine::Task { text, .. } => text }
    }

    pub fn task(&self) -> Option<&str> {
        match self { ProgramLine::Main(_) => None, ProgramLine::Task { task, .. } => Some(task) }
    }

    fn with_text(&self, text: String) -> ProgramLine {
        match self { ProgramLine::Main(_) => ProgramLine::Main(text), ProgramLine::Task { task, .. } => ProgramLine::Task { task: task.clone(), text } }
    }

    fn on(task: Option<&str>, text: String) -> ProgramLine {
        match task { Some(task) => ProgramLine::Task { task: task.into(), text }, None => ProgramLine::Main(text) }
    }
}

/// A lesson (lessons.json): its chapter, title, explanation,
/// tries, and its program.
#[derive(Deserialize, Clone, PartialEq)]
pub struct Lesson {
    pub name: String,
    pub chapter: String,
    pub title: String,
    pub text: Vec<String>,
    #[serde(default)]
    pub tries: Vec<Vec<ProgramLine>>,
    pub program: Vec<ProgramLine>,
}

/// Make `lines` on a fresh kernel, one after another (a call: then the
/// kernel run until it is quiet), each line's outcome (the Machine's) shown
/// as it goes; then a call selected: the first the lesson's text links to
/// (its subject), else the last.
/// (A run begun again cancels this one: it stops at its next await.)
pub(crate) async fn make_program(mut ui: Ui, lines: Vec<ProgramLine>) {
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
        ui.outcomes.set(m.outcomes());
        ui.console.write().push_str(&m.console());
    }
    // (The recording decoded once, the lines made: what the views read.)
    m.snapshot();
    ui.snapshot += 1;
    let outcomes = m.outcomes();
    let lesson = &lessons()[(ui.lesson)()];
    let calls = outcomes.iter().filter(|o| matches!(o, Outcome::Call { .. })).count();
    // (The lesson's own program: where its text's first link into the recording goes.)
    let linked = (lines == lesson.program).then(|| lesson.text.iter().flat_map(|p| text::pieces(p)).find_map(|x| match x {
        Piece::Link { to: Some(to @ LessonTarget::Recording { .. }), .. } => Some(to),
        _ => None,
    })).flatten();
    match linked {
        Some(to) => follow(ui, to),
        None => if let Some(k) = run_of_call(ui, calls) { ui.select(k) },
    }
}

/// The run (an index in the runs) of the program's call `n` (from 1), if the recording has it.
fn run_of_call(ui: Ui, n: usize) -> Option<usize> {
    let id = ui.outcomes.read().iter().filter_map(|o| match o {
        Outcome::Call { id, .. } => Some(*id),
        _ => None,
    }).nth(n.checked_sub(1)?)?;
    machine().run_of_call_id(id)
}

/// Follow link `target` of the lesson's text: `N` (its call N selected),
/// `N:func` (the cursor at func's first line in it), and a tab to show
/// (`:stack`, `:memory`, `:tasks`, `:console`); `try:N`, its program made
/// instead.
fn follow(mut ui: Ui, target: LessonTarget) {
    let LessonTarget::Recording { call, function, any_task, tab } = target else {
        let LessonTarget::Try { index } = target else { unreachable!() };
        if let Some(t) = lessons()[(ui.lesson)()].tries.get(index - 1) {
            ui.program.set(t.clone());
        }
        return;
    };
    let Some((k, line)) = resolve(ui, call, function.as_deref(), any_task) else { return };
    ui.select(k);
    if let Some(j) = line {
        ui.go(j);
    }
    if let Some(tab) = tab {
        ui.tab.set(tab);
    }
}

/// Where a link into the recording goes, if the recording has it: call
/// `call`'s run, and in it `func`'s first line (on any task's, `any_task`).
fn resolve(ui: Ui, call: usize, func: Option<&str>, any_task: bool) -> Option<(usize, Option<usize>)> {
    let k = run_of_call(ui, call)?;
    let Some(func) = func else { return Some((k, None)) };
    let m = machine();
    let s = m.view(|v| v.run(k))?;
    let at = m.first_call(func, s.first, s.last, (!any_task).then_some(s.task.addr))?;
    Some((k, Some(Ui::run_lines(k).iter().position(|l| l.i >= at)?)))
}

#[component]
pub fn LessonPane() -> Element {
    let mut ui = use_context::<Ui>();
    let all = lessons();
    let at = (ui.lesson)();
    let lesson = &all[at];
    let program = (ui.program)();
    let mine = program == lesson.program;
    let outcomes = ui.outcomes.read().clone();
    // (Task columns: the program's first, then those used or made.)
    let active = if ui.up() { machine().task_columns(program.len()) } else { Vec::new() };
    let mut columns: Vec<Option<String>> = vec![None];
    for task in program.iter().filter_map(|step| step.task()).chain(active.iter().map(String::as_str)) {
        if !columns.iter().any(|column| column.as_deref() == Some(task)) {
            columns.push(Some(task.into()));
        }
    }
    let many = columns.len() > 1;
    let col = |step: &ProgramLine| columns.iter().position(|column| column.as_deref() == step.task()).unwrap_or(0);
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
        // (The text, scrolling, the next lesson at its end; the program
        // under it, always in view.)
        article { class: "lesson",
            h1 { "{lesson.title}" }
            div { class: "explain", for p in lesson.text.iter() {
                ParagraphView { pieces: text::pieces(p), follow: move |target| follow(ui, target) }
            } }
            crate::nav::Turn {}
        }
        section { class: "programpane",
            div { class: "ptitle dim", "Program"
                if !mine { a { title: "the lesson's own program again, on a fresh kernel", onclick: move |_| ui.open_lesson(at), "↺ reset" } }
            }
            div { class: "program",
                if many {
                    div { class: "taskheads", style: "{grid}",
                        for (name , pid) in names.iter().cloned() { div { class: "task mono", style: "--hue: {crate::chart::hue(pid)}", "{name}" } }
                    }
                }
                ol { class: if many { "lines columns" } else { "lines" }, style: "{grid}",
                    for (j , step) in program.iter().enumerate() {
                        li { key: "{j}:{step.text()}", class: if many { "row" } else { "" },
                            if many {
                                for w in waits(j).into_iter().filter(|&w| w != col(step)) {
                                    div { class: "wait", style: "grid-column: {w + 1}; --hue: {crate::chart::hue(names[w].1)}", title: "a call in this task column, waiting (blocked)" }
                                }
                            }
                            Cell { j, step: step.clone(), outcome: outcomes.get(j).cloned(),
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
                                            done: move |text: String| if !text.is_empty() { ui.program.write().push(ProgramLine::on(task.as_deref(), text)); } }
                                    }
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
fn task_name(ui: Ui, task: Option<&str>, program: &[ProgramLine]) -> (String, i32) {
    let id = program.iter().enumerate().filter(|(_, step)| step.task() == task).find_map(|(j, _)| match ui.outcomes.read().get(j) {
        Some(Outcome::Call { id, .. }) => Some(*id),
        _ => None,
    });
    // (A call made: the kernel is up.)
    let recorded = id.and_then(|id| {
        let m = machine();
        m.run_of_call_id(id).and_then(|k| m.view(|v| v.run(k))).map(|s| s.task)
    });
    match recorded {
        Some(t) => (t.named(), t.pid()),
        None if task.is_none() => ("the program".into(), 0),
        None => (task.unwrap_or_default().into(), 0),
    }
}

/// A program's step `j` (written in place: Enter makes the program again,
/// the text changed, or removed if emptied): what it did; a
/// call, its recording selected as it gets the focus. In a task column: its
/// column, and its task's hue.
#[component]
fn Cell(j: usize, step: ProgramLine, outcome: Option<Outcome>, column: Option<(usize, i32)>) -> Element {
    let mut ui = use_context::<Ui>();
    // (A call's run, once the recording has it: drawn again as it changes.)
    (ui.snapshot)();
    let run = match outcome { Some(Outcome::Call { id, .. }) => machine().run_of_call_id(id), _ => None };
    let selected = run.is_some() && run == ui.run();
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
                    focus: move |_| if let Some(k) = run { ui.select(k) } }
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
        for step in lessons().iter().flat_map(|e| e.program.iter()) {
            if let Ok(Statement::Call { call, .. }) = statement(step.text()) && !out.contains(&call) {
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

/// A parsed lesson paragraph. A link into the recording is one while the
/// recording has where it goes (the program changed, maybe); try links
/// always.
#[component]
pub(crate) fn ParagraphView(
    pieces: Vec<Piece>,
    follow: EventHandler<LessonTarget>,
) -> Element {
    let ui = use_context::<Ui>();
    // (Read again as the recording grows.)
    (ui.snapshot)();
    let live = |to: &LessonTarget| match to {
        LessonTarget::Recording { call, function, any_task, .. } => ui.up() && resolve(ui, *call, function.as_deref(), *any_task).is_some(),
        LessonTarget::Try { .. } => true,
    };
    rsx! {
        p {
            for piece in pieces {
                match piece {
                    Piece::Text(text) => rsx! { "{text}" },
                    Piece::Code(code) => rsx! { code { "{code}" } },
                    Piece::Link { label, to, .. } => match to {
                        Some(to) if live(&to) => {
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
