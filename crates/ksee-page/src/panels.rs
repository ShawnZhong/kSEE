//! Beside the source, a tab each: the Stack (stack.rs); the tasks at the
//! moment shown; what the call selected did to the kernel's data (Objects);
//! what it asked of the disk (Disk); the kernel's console.

use std::collections::BTreeSet;
use std::rc::Rc;

use dioxus::prelude::*;
use ksee_core::view::{RegionKind, TaskState};

use crate::machine;
use crate::objects::{ObjectDefinition, ObjectsView};
use crate::state::{Derived, Tab, Ui};

#[component]
pub fn Tabs() -> Element {
    let mut ui = use_context::<Ui>();
    let tab = (ui.tab)();
    let head = |t: Tab, label: &'static str, title: &'static str| {
        rsx! {
            a { class: if tab == t { "tab on" } else { "tab" }, title, onclick: move |_| ui.tab.set(t), "{label}" }
        }
    };
    rsx! {
        div { class: "panels",
            nav { class: "tabs",
                {head(Tab::Stack, "Stack", "the calls at the cursor, their values as they were then")}
                {head(Tab::Tasks, "Tasks", "every task at the cursor: what each is doing, its task_struct")}
                {head(Tab::Objects, "Objects", "the objects the call selected changed, and how they point to each other")}
                {head(Tab::Disk, "Disk", "what the call selected asked of the disk: each transfer, and what it wrote")}
                {head(Tab::Console, "Console", "the kernel's log and the programs' output")}
            }
            match tab {
                Tab::Stack => rsx! { div { class: "pane", crate::stack::Stack {} } },
                Tab::Objects => rsx! { div { class: "pane", Objects {} } },
                Tab::Tasks => rsx! { div { class: "pane", Tasks {} } },
                Tab::Disk => rsx! { div { class: "pane", Disk {} } },
                Tab::Console => rsx! { div { class: "pane", pre { class: "console", "{ui.console}" } } },
            }
        }
    }
}

/// Query the selected call's object graph and adapt its semantic actions to
/// the browser-backed page state.
#[component]
fn Objects() -> Element {
    let mut ui = use_context::<Ui>();
    let derived = use_context::<Derived>();
    let graph = use_memo(move || {
        (ui.snapshot)();
        let machine = machine();
        let nodes = derived.syscall.cloned().map_or_else(Vec::new, |syscall| {
            machine.view(|view| view.objects(syscall.first, syscall.last))
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
            on_record: move |record| ui.go_record(derived, record),
            on_definition: move |definition: ObjectDefinition| ui.seen.set(Some((definition.file, definition.line))),
            on_object: show,
        }
    }
}

/// Box `k` in view.
fn show(k: usize) {
    if let Some(e) = crate::chart::element(&format!("obj{k}")) {
        e.scroll_into_view();
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
                    let place = t.doing.as_ref().map_or(String::new(), |s| format!(" in {s}"));
                    let class = format!("frame{}{}", if running { " on" } else { "" }, if matches!(t.state, TaskState::Later | TaskState::Exited) { " later" } else { "" });
                    let fields = if is_open { m.view(|v| v.task_fields(t.task.addr, at)) } else { Vec::new() };
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

/// What the call selected asked of the disk: each transfer, in order (a
/// click: the cursor where it was handed over; "done": where the disk made
/// it), the one under way at the cursor marked; a write, the bytes it
/// changed, a read, the bytes it read.
#[component]
fn Disk() -> Element {
    let ui = use_context::<Ui>();
    let d = use_context::<Derived>();
    (ui.snapshot)();
    let Some(s) = d.syscall.cloned() else {
        return rsx! { p { class: "pad dim", "Select a call: what it asked of the disk." } };
    };
    let all = machine().disk(s.first, s.last);
    if all.is_empty() {
        return rsx! { p { class: "pad dim", "{s.name} by {s.task.named()}: nothing went to the disk." } };
    }
    let cursor = d.line.cloned().map_or(0, |l| l.at);
    let head = format!("{} transfer{} while {} by {} ran (the tag: its request)", all.len(), if all.len() == 1 { "" } else { "s" }, s.name, s.task.named());
    rsx! {
        div { class: "objects disk",
            div { class: "ohead", "{head}" }
            for x in all {
                {
                    let t = &x.t;
                    let on = x.asked <= cursor && cursor < x.done;
                    let what = format!("{} {} at sector {} · tag {}", if t.write { "write" } else { "read" }, size(t.len), t.sector, t.tag);
                    let (asked, done) = (x.asked, x.done);
                    let bytes = if t.write { changed(&t.before, &t.after) } else { format!("read: {}", hex(&t.after[..t.after.len().min(16)])) };
                    rsx! {
                        div { class: if on { "xfer on" } else { "xfer" },
                            a { class: "mono", title: "where it was handed to the disk (queue_rq)", onclick: move |_| ui.go_record(d, asked), "{what}" }
                            " · "
                            a { class: "dim", title: "where the disk made it (then its interrupt)", onclick: move |_| ui.go_record(d, done), "done" }
                            div { class: "mono dim bytes", "{bytes}" }
                        }
                    }
                }
            }
        }
    }
}

/// `n` bytes, said: "512 B", "4 KiB".
fn size(n: u32) -> String {
    if n >= 1024 && n % 1024 == 0 { format!("{} KiB", n / 1024) } else { format!("{n} B") }
}

/// Bytes in hex, a space each.
fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ")
}

/// What a write changed: how many bytes, and the first run of them, from
/// what to what (16 at most).
fn changed(before: &[u8], after: &[u8]) -> String {
    let diff: Vec<usize> = (0..before.len().min(after.len())).filter(|&k| before[k] != after[k]).collect();
    let Some(&first) = diff.first() else { return "wrote what was there".into() };
    let end = (first..before.len().min(after.len())).take_while(|&k| before[k] != after[k]).last().unwrap_or(first) + 1;
    let end = end.min(first + 16);
    format!("{} byte{} changed; at {first}: {} → {}", diff.len(), if diff.len() == 1 { "" } else { "s" }, hex(&before[first..end]), hex(&after[first..end]))
}
