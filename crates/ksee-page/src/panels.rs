//! Beside the source, a tab each: the Stack (stack.rs); the tasks at the
//! moment shown; what the run selected wrote in the kernel's memory (Memory);
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
                {head(Tab::Memory, "Memory", "what the run selected wrote in the kernel's memory: the objects it changed, and how they point to each other")}
                {head(Tab::Disk, "Disk", "every transfer the disk made during the calls, for which task, and what it wrote; the cursor's place among them")}
                {head(Tab::Console, "Console", "the kernel's log and the programs' output")}
            }
            div { class: "pane",
                match tab {
                    Tab::Stack => rsx! { crate::stack::Stack {} },
                    Tab::Memory => rsx! { Objects {} },
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

/// The tasks at the moment shown, newest first, a line each: what it is
/// doing and where, the one running marked; a click selects one (at first: the one
/// running), its detail under its line (TaskDetail).
#[component]
fn Tasks() -> Element {
    let ui = use_context::<Ui>();
    (ui.snapshot)();
    let d = use_context::<Derived>();
    let mut chosen = use_signal(|| None::<u32>);
    let at = d.line.cloned().map_or(0, |l| l.at);
    let m = machine();
    let list = m.view(|v| v.tasks(at));
    // (Newest first: the program's, then the kernel's own; those not made
    // yet last.)
    let mut order: Vec<usize> = (0..list.len()).collect();
    order.sort_by_key(|&k| (list[k].state == TaskState::Later, std::cmp::Reverse(list[k].task.pid())));
    // (The one selected: the one chosen, if the list has it; else the one running.)
    let sel = chosen().filter(|&a| list.iter().any(|t| t.task.addr == a)).or_else(|| list.iter().find(|t| t.state == TaskState::Running).map(|t| t.task.addr));
    let selected = sel.and_then(|a| list.iter().find(|t| t.task.addr == a));
    // (Its fd table: its own, or the oldest task's that has it, shared, if
    // it has files: a kernel thread's, the empty one they share, is not said.)
    let (files, shared) = selected.and_then(|t| t.files).map_or((Vec::new(), None), |f| {
        let files = m.view(|v| v.open_files(f, at));
        let first = order.iter().map(|&k| &list[k]).filter(|t| t.files == Some(f)).last();
        let shared = first.filter(|o| Some(o.task.addr) != sel && !files.is_empty()).map(|o| o.task.named());
        (files, shared)
    });
    rsx! {
        table { class: "fields",
            for k in order {
                {
                    let t = &list[k];
                    let (said, title) = state(t.state);
                    let place = t.doing.as_ref().map_or(String::new(), |s| format!(" in {s}"));
                    let on = Some(t.task.addr) == sel;
                    let class = format!("frame opens{}{}", if on { " on" } else { "" },
                                        if matches!(t.state, TaskState::Later | TaskState::Exited) { " later" } else { "" });
                    let (addr, name) = (t.task.addr, t.task.named());
                    rsx! {
                        tr { class, title, onclick: move |_| chosen.set(Some(addr)),
                            td { colspan: "3",
                                a { class: "mono", "{name}" }
                                span { class: "dim", "  {said}{place}" }
                            }
                        }
                        if on { TaskDetail { addr, files: files.clone(), shared: shared.clone(), at } }
                    }
                }
            }
        }
    }
}

/// Under a task's line (Tasks), the task at `addr`: its open files, `files`
/// (or whose it shares), each a click from its struct file; and its
/// task_struct, folded; as they were after record `at`.
#[component]
fn TaskDetail(addr: u32, files: Vec<ksee_core::view::OpenFile>, shared: Option<String>, at: usize) -> Element {
    let mut open = use_context::<Signal<BTreeSet<String>>>();
    let m = machine();
    let raw = format!("task/{addr}/task_struct");
    let raw_open = open.read().contains(&raw);
    let fields = if raw_open { m.view(|v| v.task_fields(addr, at)) } else { Vec::new() };
    rsx! {
        // (As the Stack's values: a name, a value, a type; a click opens one.)
        if let Some(s) = &shared {
            tr { title: "its fd table is another task's (a thread's, its process's)",
                td { class: "fname mono", "fds" } td { class: "fval", colspan: "2", "{s}'s, shared" } }
        }
        for f in files.into_iter().filter(|_| shared.is_none()) {
            {
                // (A click: its struct file, as it was.)
                let path = format!("task/{addr}/fd{}", f.fd);
                let file_open = open.read().contains(&path);
                let fields = if file_open { m.view(|v| v.file_fields(f.file, at)) } else { Vec::new() };
                let pp = path.clone();
                rsx! {
                    tr { class: "opens", title: "task->files->fdt->fd[{f.fd}]: an open file, the struct file at {f.file:#x} (the same address: the same open file; a click: its fields)",
                        onclick: move |_| { let mut s = open.write(); if !s.remove(&pp) { s.insert(pp.clone()); } },
                        td { class: "fname mono", span { class: "tw", if file_open { "▾ " } else { "▸ " } } "fd {f.fd}" }
                        td { class: "fval mono", "{f.name}" span { class: "dim", " · {f.mode} · position {f.pos}" } }
                        td { class: "ftype mono", "{f.file:#x}" }
                    }
                    for o in fields {
                        crate::stack::Value { o: o.clone(), path: format!("{path}.{}", o.name), depth: 1, at }
                    }
                }
            }
        }
        tr { class: "opens", title: "the task's task_struct, as it was (a click: its fields)",
            onclick: move |_| { let mut s = open.write(); if !s.remove(&raw) { s.insert(raw.clone()); } },
            td { class: "fname mono", span { class: "tw", if raw_open { "▾ " } else { "▸ " } } "task_struct" }
            td { class: "fval mono", "{addr:#x}" }
            td { class: "ftype mono", "struct task_struct" }
        }
        for o in fields {
            crate::stack::Value { o: o.clone(), path: format!("task/{addr}.{}", o.name), depth: 1, at }
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
