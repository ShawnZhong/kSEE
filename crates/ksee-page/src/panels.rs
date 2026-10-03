//! Beside the source, a tab each: the Stack (stack.rs); the tasks at the
//! moment shown; what the call selected did to the kernel's data (Objects);
//! the kernel's console.

use std::collections::BTreeSet;
use std::rc::Rc;

use dioxus::prelude::*;
use ksee_core::view::{RegionKind, TaskState};

use crate::machine;
use crate::objects_view::{ObjectDefinition, ObjectsView};
use crate::state::{Derived, Tab, Ui};

#[component]
pub fn Tabs() -> Element {
    let ui = use_context::<Ui>();
    use_context_provider(|| Signal::new(BTreeSet::<String>::new()));
    let tab = (ui.tab)();
    let head = |t: Tab, label: &'static str, title: &'static str| {
        rsx! {
            a { class: if tab == t { "tab on" } else { "tab" }, title, onclick: move |_| ui.set_tab(t), "{label}" }
        }
    };
    rsx! {
        div { class: "panels",
            nav { class: "tabs",
                {head(Tab::Stack, "Stack", "the calls at the cursor, their values as they were then")}
                {head(Tab::Tasks, "Tasks", "every task at the cursor: what each is doing, its task_struct")}
                {head(Tab::Objects, "Objects", "the objects the call selected changed, and how they point to each other")}
                {head(Tab::Console, "Console", "the kernel's log and the programs' output")}
            }
            match tab {
                Tab::Stack => rsx! { div { class: "pane", crate::stack::Stack {} } },
                Tab::Objects => rsx! { div { class: "pane", Objects {} } },
                Tab::Tasks => rsx! { div { class: "pane", Tasks {} } },
                Tab::Console => rsx! { div { class: "pane", pre { class: "console", "{ui.console}" } } },
            }
        }
    }
}

/// Query the selected call's object graph and adapt its semantic actions to
/// the browser-backed page state.
#[component]
fn Objects() -> Element {
    let ui = use_context::<Ui>();
    let derived = use_context::<Derived>();
    let graph = use_memo(move || {
        (ui.snapshot)();
        let machine = machine();
        let nodes = derived.syscall.cloned().map_or_else(Vec::new, |syscall| {
            machine.view(|view| view.objects(
                syscall.first,
                syscall.last.unwrap_or(machine.records().saturating_sub(1)),
            ))
        });
        let definitions: Vec<Option<ObjectDefinition>> = nodes.iter().map(|node| {
            (node.kind == RegionKind::Global)
                .then(|| machine.view(|view| view.definition(&node.what)))
                .flatten()
                .map(|place| ObjectDefinition { file: place.file, line: place.line })
        }).collect();
        (Rc::new(nodes), Rc::new(definitions))
    });
    let Some(syscall) = derived.syscall.cloned() else {
        return rsx! { p { class: "pad dim", "Select a call: what it did to the kernel's data." } };
    };
    let title = format!("{} by {}", syscall.name, syscall.task.named());
    let cursor = derived.line.cloned().map(|line| line.at);
    rsx! {
        ObjectsView {
            nodes: graph.read().0.clone(),
            title,
            cursor,
            definitions: graph.read().1.clone(),
            on_record: move |record| ui.go_record(derived, record, None),
            on_definition: move |definition: ObjectDefinition| ui.show_definition(Some((definition.file, definition.line))),
            on_object: show,
        }
    }
}

/// Box `k` in view.
fn show(k: usize) {
    if let Some(element) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id(&format!("obj{k}")))
    {
        element.scroll_into_view();
    }
}

/// What a task's state says, hovered.
fn state(s: TaskState) -> (&'static str, &'static str) {
    match s {
        TaskState::Running => ("running", "running at the cursor: on the CPU"),
        TaskState::Runnable => ("runnable", "on the run queue: waiting for the CPU"),
        TaskState::Sleeping => (
            "sleeping",
            "off the run queue: waiting for something (its syscall or call says what)",
        ),
        TaskState::Exited => (
            "exited",
            "exited by the cursor: the kernel's task list no longer has it",
        ),
        TaskState::Later => ("later", "made only after the cursor"),
    }
}

/// The tasks at the moment shown, as a tree (a thread under its process's
/// leader, a process under its parent): the one running marked, and open
/// to begin with; each what it is doing and where; open, its task_struct
/// as it was then.
#[component]
fn Tasks() -> Element {
    let ui = use_context::<Ui>();
    (ui.snapshot)();
    let d = use_context::<Derived>();
    let mut open = use_context::<Signal<BTreeSet<String>>>();
    let at = d.line.cloned().map_or(0, |l| l.at);
    let m = machine();
    let list = m.view(|v| v.tasks(at));
    // (Each under the task it sits under, if the list has it; siblings by pid.)
    let known: Vec<u32> = list.iter().map(|t| t.task.addr).collect();
    let up = |t: &ksee_core::view::TaskAt| t.up.filter(|u| known.contains(u));
    let mut order = Vec::new();
    // (Depth first: a task, then those under it.)
    fn visit(
        list: &[ksee_core::view::TaskAt],
        up: &dyn Fn(&ksee_core::view::TaskAt) -> Option<u32>,
        at: Option<u32>,
        depth: usize,
        out: &mut Vec<(usize, usize)>,
    ) {
        let mut kids: Vec<usize> = (0..list.len()).filter(|&k| up(&list[k]) == at).collect();
        kids.sort_by_key(|&k| list[k].task.pid.unwrap_or(0));
        for k in kids {
            out.push((k, depth));
            visit(list, up, Some(list[k].task.addr), depth + 1, out);
        }
    }
    visit(&list, &up, None, 0, &mut order);
    rsx! {
        table { class: "objs",
            for (k , depth) in order {
                {
                    let t = &list[k];
                    let running = t.state == TaskState::Running;
                    // (Open: the running one unless folded, another if opened.)
                    let p = format!("task/{}", t.task.addr);
                    let is_open = running != open.read().contains(&p);
                    let (said, title) = state(t.state);
                    let place = if t.last.is_some() { t.syscall.clone().or(t.func.clone()).map_or(String::new(), |s| format!(" in {s}")) } else { String::new() };
                    let class = format!("frame{}{}", if running { " on" } else { "" }, if matches!(t.state, TaskState::Later | TaskState::Exited) { " later" } else { "" });
                    let fields = if is_open { m.at_moment(at, |v, mem| v.task_fields(mem, t.task.addr)).fields } else { Vec::new() };
                    let name = t.task.named();
                    let q = p.clone();
                    rsx! {
                        tr { class: "{class} opens", title,
                            onclick: move |_| { let mut s = open.write(); if !s.remove(&q) { s.insert(q.clone()); } },
                            td { colspan: "3", style: "padding-left: {8 + depth * 16}px",
                                a { class: "mono", title: if is_open { "fold it" } else { "its task_struct, as it was at the cursor" },
                                    span { class: "tw", if is_open { "▾ " } else { "▸ " } }
                                    "{name}" }
                                span { class: "dim", "  {said}{place}" }
                            }
                        }
                        for o in fields {
                            crate::stack::Value { o: o.clone(), path: format!("{p}.{}", o.name), depth: 1, at }
                        }
                    }
                }
            }
        }
    }
}
