//! The moment shown: the transport (where it is, the steps), and the
//! Stack: the calls then, innermost first, each with its arguments and
//! locals as memory was then; a value opens into its members, or what it
//! points to, and says who last wrote it.

use std::collections::BTreeSet;

use dioxus::prelude::*;
use ksee_core::view::{step, Frame, Object};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;

use crate::machine;
use crate::state::{Derived, Moment, Shared, Tab, Ui};

/// The stack at the moment shown (the pointer's over the chart, else the
/// cursor's).
pub fn frames() -> Shared<Vec<Frame>> {
    use_context::<Derived>().stack.cloned()
}

/// The frame shown: the one chosen (or under the pointer), else the innermost.
pub fn shown() -> Option<Frame> {
    let d = use_context::<Derived>();
    let k = d.frame.cloned();
    let frames = d.stack.read();
    frames.get(k).or(frames.first()).cloned()
}

/// The record the moment shown is at.
pub fn moment() -> usize {
    use_context::<Derived>().moment.cloned().map_or(0, |(i, _)| i)
}

/// A step from the cursor (`dir`: on, 1, or back, -1): past the call's
/// lines, into its task's syscall after it (or before). The lines read
/// from the Machine (the window's listener calls it, outside the page).
pub fn walk(ui: Ui, dir: isize, over: bool) {
    let Some(at) = ui.at.peek().clone() else { return };
    let k = at.call;
    let lines = Ui::lines_of(k);
    if let Some(j) = step(&lines, at.line, dir, over) {
        ui.go(j);
        return;
    }
    let task = machine().view(|v| v.syscalls().into_iter().nth(k).map(|s| s.task.addr));
    let Some(task) = task else { return };
    let next = machine().view(|v| {
        let all = v.syscalls();
        let mine: Vec<usize> = (0..all.len()).filter(|&j| all[j].task.addr == task).collect();
        let at = mine.iter().position(|&j| j == k)?;
        mine.get(at.checked_add_signed(dir)?).copied()
    });
    if let Some(next) = next {
        ui.select(next);
        if dir < 0 {
            ui.go(Ui::lines_of(next).len().saturating_sub(1));
        }
    }
}

/// The frame chosen on the cursor's stack (`d.stack`), `dir` up it (-1:
/// the callee) or down (1: the caller).
pub fn frame(mut ui: Ui, d: Derived, dir: isize) {
    let Some(at) = ui.at.peek().clone() else { return };
    let frames = d.stack.peek();
    let k = d.frame.peek().saturating_add_signed(dir).min(frames.len().saturating_sub(1));
    ui.at.set(Some(Moment { frame: frames.get(k).map(|f| (f.first, f.func.clone())), ..at }));
}

/// The page's keys, anywhere on it: one listener on the window; ← → a
/// line (Shift: over the calls); ↑ ↓ the frame
/// up (its callee) or down (its caller). Not in text fields (the lesson's
/// lines, find): arrows move the cursor there.
pub fn listen(ui: Ui) {
    let d = use_context::<Derived>();
    use_hook(move || {
        let Some(window) = web_sys::window() else { return };
        let document = window.document();
        let on = Closure::<dyn Fn(web_sys::KeyboardEvent)>::new(move |e: web_sys::KeyboardEvent| {
            // (A field's own keys first: the editor's completion takes the arrows.)
            if e.default_prevented() {
                return;
            }
            if let Some(active) = document.as_ref().and_then(|d| d.active_element()) {
                let tag = active.tag_name();
                if tag == "TEXTAREA" || tag == "INPUT" {
                    return;
                }
            }
            let mut peek = ui.peek;
            peek.set(None);
            let over = e.shift_key();
            let arrow = match e.key().as_str() {
                "ArrowLeft" => { walk(ui, -1, over); true }
                "ArrowRight" => { walk(ui, 1, over); true }
                "ArrowUp" => { frame(ui, d, -1); true }
                "ArrowDown" => { frame(ui, d, 1); true }
                _ => false,
            };
            if arrow {
                e.prevent_default();
            }
        });
        let _ = window.add_event_listener_with_callback("keydown", on.as_ref().unchecked_ref());
        // (The page's life: never removed.)
        on.forget();
    });
}

/// The calls at the moment shown, innermost first, at the bottom of the
/// pane: the frame chosen open, its arguments and locals as memory was
/// then; a click on another, it chosen. Then the task running.
#[component]
pub fn Stack() -> Element {
    let mut ui = use_context::<Ui>();
    let frames = frames();
    let chosen = use_context::<Derived>().frame.cloned();
    let i = moment();
    let task = machine().view(|v| v.rec.tasks.get(i).map(|&t| v.task(t, i)));
    let mut open = use_context::<Signal<BTreeSet<String>>>();
    rsx! {
        div { class: "stack",
            table { class: "objs",
                for (k , f) in frames.iter().cloned().enumerate() {
                    tr { class: if k == chosen { "frame on" } else { "frame" }, onclick: {
                        let id = (f.first, f.func.clone());
                        move |_| {
                            let at = ui.at.peek().clone();
                            if let Some(at) = at { ui.peek.set(None); ui.at.set(Some(Moment { frame: Some(id.clone()), ..at })) }
                        }
                    },
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
                        "{t.named()}" }
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
