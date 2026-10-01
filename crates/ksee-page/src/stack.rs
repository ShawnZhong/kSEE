//! The moment shown: the transport (where it is, the steps), and the
//! Stack: the calls then, innermost first, each with its arguments and
//! locals as memory was then; a value opens into its members, or what it
//! points to, and says who last wrote it.

use std::collections::BTreeSet;

use dioxus::prelude::*;
use ksee_core::view::{step, Frame, Move, Object};

use crate::machine;
use crate::state::{Derived, Shared, Tab, Ui};

/// The stack at the moment shown (the pointer's over the chart, else the
/// cursor's).
pub fn frames() -> Shared<Vec<Frame>> {
    use_context::<Derived>().stack.cloned()
}

/// The frame shown: the one chosen (or under the pointer), else the innermost.
pub fn shown() -> Option<Frame> {
    let d = use_context::<Derived>();
    let k = d.shown.cloned();
    let frames = d.stack.read();
    frames.get(k).or(frames.first()).cloned()
}

/// The record the moment shown is at.
pub fn moment() -> usize {
    use_context::<Derived>().moment.cloned().map_or(0, |(i, _)| i)
}

/// A step from the cursor (`dir`: on, 1, or back, -1): past the call's
/// lines, into its task's syscall after it (or before).
pub fn walk(ui: Ui, dir: isize, how: Move) {
    let lines = ui.lines();
    if let Some(j) = step(&lines, (ui.cursor)(), dir, how) {
        ui.go(j);
        return;
    }
    let (Some(s), Some(k)) = (ui.syscall(), (ui.call)()) else { return };
    let next = machine().view(|v| {
        let all = v.syscalls();
        let mine: Vec<usize> = (0..all.len()).filter(|&j| all[j].task.addr == s.task.addr).collect();
        let at = mine.iter().position(|&j| j == k)?;
        mine.get(at.checked_add_signed(dir)?).copied()
    });
    if let Some(next) = next {
        ui.select(next);
        if dir < 0 {
            ui.go(ui.lines().len().saturating_sub(1));
        }
    }
}

/// The frame chosen, `dir` up the Stack (-1: the callee) or down (1: the caller).
pub fn frame(mut ui: Ui, dir: isize) {
    let n = consume_context::<Derived>().stack.read().len();
    let k = (ui.frame)().unwrap_or(0);
    ui.frame.set(Some(k.saturating_add_signed(dir).min(n.saturating_sub(1))));
}

/// The work area's keys: ← → a line (Shift: over the calls; Alt: out of
/// the function); ↑ ↓ the frame up (its callee) or down (its caller).
pub fn key(ui: Ui, e: &KeyboardEvent) {
    let m = e.modifiers();
    let how = if m.alt() { Move::Out } else if m.shift() { Move::Over } else { Move::Into };
    match e.key() {
        Key::ArrowLeft => walk(ui, -1, how),
        Key::ArrowRight => walk(ui, 1, how),
        Key::ArrowUp => frame(ui, -1),
        Key::ArrowDown => frame(ui, 1),
        _ => return,
    }
    e.prevent_default();
}

/// The calls at the moment shown, innermost first, at the bottom of the
/// pane: the frame chosen open, its arguments and locals as memory was
/// then; a click on another, it chosen. Then the task running.
#[component]
pub fn Stack() -> Element {
    let mut ui = use_context::<Ui>();
    let frames = frames();
    let chosen = use_context::<Derived>().shown.cloned();
    let i = moment();
    let task = machine().view(|v| v.rec.tasks.get(i).map(|&t| v.task(t, i)));
    let mut open = use_context::<Signal<BTreeSet<String>>>();
    rsx! {
        div { class: "stack",
            table { class: "objs",
                for (k , f) in frames.iter().cloned().enumerate() {
                    tr { class: if k == chosen { "frame on" } else { "frame" }, onclick: move |_| ui.frame.set(Some(k)),
                        td { colspan: "3",
                            a { class: "mono", "{f.func}" }
                            span { class: "dim mono", " {f.file.clone().unwrap_or_default()}:{f.line.unwrap_or(0)}" }
                            if f.inlined { span { class: "dim", " · inlined" } }
                            if let Some(s) = &f.syscall { span { class: "dim", " · {s}" } }
                        }
                    }
                    if k == chosen {
                        for o in f.args.clone().unwrap_or_default().into_iter().chain(f.locals.clone().unwrap_or_default()) {
                            Value { o: o.clone(), path: format!("{k}.{}", o.name), depth: 1, i }
                        }
                    }
                }
            }
            if let Some(t) = task {
                div { class: "current dim", "current task: "
                    a { class: "mono", title: "its task_struct, in Tasks (the task running: open there)",
                        onclick: move |_| { ui.tab.set(Tab::Tasks); open.write().remove(&format!("task/{}", t.addr)); },
                        "{crate::chart::named(&t)}" }
                }
            }
        }
    }
}

/// Who last wrote the `len` bytes at `addr`, as memory was after record
/// `i`, and back through the copies that brought them there.
fn origin(addr: u32, len: u32, i: usize) -> String {
    let m = machine();
    m.view(|v| {
        let way = v.origin(addr, len, i, 8);
        if way.is_empty() {
            return "not written in the recording".into();
        }
        let place = |w: &ksee_core::view::Write| w.at.as_ref().map_or("?".into(), |l| format!("{} {}:{}", l.func, l.file, l.line));
        let mut out = vec![format!("written at {}, by {}({}); was {}", place(&way[0]), way[0].task.comm.clone().unwrap_or_default(), way[0].task.pid.unwrap_or(0), way[0].old)];
        out.extend(way.iter().skip(1).map(|w| format!("  ← copied from {:#x}, written at {}", w.addr, place(w))));
        if let Some(from) = way.last().and_then(|w| w.from) {
            out.push(format!("  ← copied from {from:#x}, not written in the recording"));
        }
        out.join("\n")
    })
}

/// A value's row, and, opened, its members' (read as memory was at record
/// `i`); hovered, who last wrote it.
#[component]
pub fn Value(o: Object, path: String, depth: usize, i: usize) -> Element {
    let mut open = use_context::<Signal<BTreeSet<String>>>();
    let mut wrote = use_signal(String::new);
    let is_open = open.read().contains(&path);
    let can = o.children.is_some() || o.open.is_some();
    let kids = if is_open {
        o.children.clone().or_else(|| o.open.and_then(|t| machine().at_moment(i, |v, mem| v.tables.types.object(mem, &o.name, t.addr, t.ty, 1).children)))
    } else {
        None
    };
    let p = path.clone();
    let value = o.value.clone().unwrap_or_else(|| "{…}".into());
    let (addr, size) = (o.addr, o.size.clamp(1, 8));
    rsx! {
        tr { class: if o.none.is_some() { "none" } else { "" },
            td { class: "fname", style: "padding-left: {depth * 14}px",
                if can { a { class: "tw", onclick: move |_| { let mut s = open.write(); if !s.remove(&p) { s.insert(p.clone()); } }, if is_open { "▾ " } else { "▸ " } } }
                "{o.name}"
            }
            td { class: "fval", title: "{wrote}",
                onmouseenter: move |_| if addr != 0 { wrote.set(origin(addr, size, i)) },
                "{value}" }
            td { class: "ftype", "{o.ty}" }
        }
        for c in kids.unwrap_or_default() {
            Value { o: c.clone(), path: format!("{path}.{}", c.name), depth: depth + 1, i }
        }
    }
}
