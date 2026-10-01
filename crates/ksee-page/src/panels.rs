//! Beside the source: the Stack, what the call selected changed (Objects),
//! the tasks at the cursor, and the kernel's console.

use std::collections::BTreeSet;

use dioxus::prelude::*;
use ksee_core::view::TaskState;

use crate::machine;
use crate::state::{Tab, Ui};

#[component]
pub fn Tabs() -> Element {
    let mut ui = use_context::<Ui>();
    use_context_provider(|| Signal::new(BTreeSet::<String>::new()));
    let tab = (ui.tab)();
    let head = |t: Tab, label: &'static str| rsx! { a { class: if tab == t { "tab on" } else { "tab" }, onclick: move |_| ui.tab.set(t), "{label}" } };
    rsx! {
        div { class: "panels",
            nav { class: "tabs", {head(Tab::Stack, "Stack")} {head(Tab::Objects, "Objects")} {head(Tab::Tasks, "Tasks")} {head(Tab::Console, "Console")} }
            div { class: "pane",
                match tab {
                    Tab::Stack => rsx! { crate::stack::Stack {} },
                    Tab::Objects => rsx! { Objects {} },
                    Tab::Tasks => rsx! { Tasks {} },
                    Tab::Console => rsx! { pre { class: "console", "{ui.console}" } },
                }
            }
        }
    }
}

/// What the call selected did to the kernel's data: each object it
/// changed, each member from what to what.
#[component]
fn Objects() -> Element {
    let ui = use_context::<Ui>();
    let Some(s) = ui.syscall() else { return rsx! {} };
    let m = machine();
    let nodes = m.view(|v| v.objects(s.first, s.last.unwrap_or(m.records().saturating_sub(1))));
    rsx! {
        for n in nodes {
            div { class: if n.under.is_some() { "obj under" } else { "obj" },
                div { class: "fhead", b { "{n.label}" } span { class: "dim mono", " {n.what}" } }
                table { class: "vals",
                    for d in n.changes {
                        tr { td { class: "mono", "{d.member}" } td { class: "mono val", "{d.before} → {d.after}" } }
                    }
                }
            }
        }
    }
}

/// The tasks at the cursor: who each is, what it is doing, and its
/// task_struct (opened: its fields, the key ones first, as memory was then).
#[component]
fn Tasks() -> Element {
    let ui = use_context::<Ui>();
    let i = ui.lines().get((ui.cursor)()).map_or(0, |l| l.i);
    let m = machine();
    let tasks = m.view(|v| v.tasks(i));
    rsx! {
        for t in tasks {
            {
                let fields = m.at_moment(i, |v, mem| v.task_fields(mem, t.task.addr)).fields;
                let task = ksee_core::view::Object { name: format!("{}({})", t.task.comm.clone().unwrap_or_default(), t.task.pid.unwrap_or(0)),
                    ty: "struct task_struct".into(), children: Some(fields), ..Default::default() };
                rsx! {
                    div { class: "obj",
                        div { class: "fhead", span { class: "dim", "{state(t.state)} " } span { class: "dim mono", "{t.syscall.clone().unwrap_or_default()} {t.func.clone().unwrap_or_default()}" } }
                        table { class: "vals", crate::stack::Value { o: task, path: format!("task/{}", t.task.addr), depth: 0, i } }
                    }
                }
            }
        }
    }
}

fn state(s: TaskState) -> &'static str {
    match s {
        TaskState::Running => "running",
        TaskState::Runnable => "runnable",
        TaskState::Sleeping => "sleeping",
        TaskState::Exited => "exited",
        _ => "not yet",
    }
}
