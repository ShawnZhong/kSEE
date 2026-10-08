//! Beside the source, a tab each: the Stack (stack.rs); the tasks at the
//! moment shown; what the run selected did to the kernel's data (Objects);
//! what it asked of the disk (Disk); the kernel's console.

use std::collections::BTreeSet;
use std::rc::Rc;

use dioxus::prelude::*;
use wasm_bindgen::JsCast;
use ksee_core::view::TaskState;

use crate::machine;
use crate::objects::Objects;
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
                {head(Tab::Objects, "Objects", "the objects the run selected changed, and how they point to each other")}
                {head(Tab::Disk, "Disk", "every transfer the disk made during the calls, for which task, and what it wrote; the cursor's place among them")}
                {head(Tab::Console, "Console", "the kernel's log and the programs' output")}
            }
            div { class: "pane",
                match tab {
                    Tab::Stack => rsx! { crate::stack::Stack {} },
                    Tab::Objects => rsx! { Objects {} },
                    Tab::Tasks => rsx! { Tasks {} },
                    Tab::Disk => rsx! { Disk {} },
                    Tab::Console => rsx! { pre { class: "console", "{ui.console}" } },
                }
            }
        }
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
        table { class: "fields",
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

/// The disk's history: every transfer it made, in order, for whichever
/// task (a device is shared: a process's fsync, jbd2's journal, a worker's
/// writeback); at the cursor's moment, those made before it plain, the one
/// under way marked (kept in view), those not handed over yet gray. A row:
/// what it did, its size and where on the disk, by which task (the one
/// running as the driver handed it over: a device's view, what the bytes
/// are, the file system's, it does not say; a click: the
/// cursor just after it was handed over; "done": where the kernel took it
/// as done, in the disk's interrupt); a
/// write's changed bytes, a read's.
#[component]
fn Disk() -> Element {
    let ui = use_context::<Ui>();
    let d = use_context::<Derived>();
    // (Kept in view: the row under way, else the last made.)
    use_effect(move || {
        let _ = d.line.read();
        let doc = crate::document();
        let last_made = || doc.as_ref()?.query_selector_all(".disk .xfer.made").ok().and_then(|l| l.item(l.length().checked_sub(1)?));
        let row = doc.as_ref().and_then(|d| d.query_selector(".disk .xfer.on").ok().flatten()).or_else(|| last_made().and_then(|n| n.dyn_into().ok()));
        if let Some(e) = row {
            let o = web_sys::ScrollIntoViewOptions::new();
            o.set_block(web_sys::ScrollLogicalPosition::Nearest);
            e.scroll_into_view_with_scroll_into_view_options(&o);
        }
    });
    // (The history, once a snapshot; the cursor only says where it is in it.)
    let history = use_memo(move || {
        (ui.snapshot)();
        crate::state::Shared(Rc::new(machine().disk()))
    });
    let all = history.read().0.clone();
    if all.is_empty() {
        return rsx! { p { class: "pad dim", "Nothing has gone to the disk yet." } };
    }
    let cursor = d.line.cloned().map_or(0, |l| l.at);
    let head = format!("{} transfers, the disk's whole history; at the cursor, gray: not handed over yet", all.len());
    rsx! {
        div { class: "listing disk",
            div { class: "listhead dim", "{head}" }
            for x in all.iter().cloned() {
                {
                    let t = &x.transfer;
                    let (asked, done) = (x.asked, x.done);
                    let class = if asked > cursor { "xfer later" } else if cursor < done { "xfer on" } else { "xfer made" };
                    let start = t.sector * 512;
                    let what = format!("{} {} at {} · by {}", if t.write { "write" } else { "read" }, size(t.len as u64), size(start), x.by);
                    let title = format!("bytes {start}–{} (sector {}); the request's tag {} (blk-mq's: how the disk says which request it did); by: the task running as it was handed over", start + t.len as u64 - 1, t.sector, t.tag);
                    let bytes = if t.write { changed(&t.before, &t.after) } else { format!("read: {}", hex(&t.after[..t.after.len().min(16)])) };
                    rsx! {
                        div { class, title: "a click: where it was handed to the disk (queue_rq)", onclick: move |_| ui.go_record(asked),
                            a { class: "mono", title: "{title}", "{what}" }
                            " · "
                            a { class: "dim", title: "where the kernel took it as done (the disk's interrupt)",
                                onclick: move |e| { e.stop_propagation(); ui.go_record(done) }, "done" }
                            div { class: "mono dim bytes", "{bytes}" }
                        }
                    }
                }
            }
        }
    }
}

/// `n` bytes, said in KB (1024 bytes) when whole ones: "4 KB", "512 B".
fn size(n: u64) -> String {
    if n % 1024 == 0 { format!("{} KB", n / 1024) } else { format!("{n} B") }
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
