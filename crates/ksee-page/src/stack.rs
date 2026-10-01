//! The moment at the cursor: the transport (where it is, the steps), and
//! the Stack: the calls then, innermost first, each with its arguments and
//! locals as memory was then; a value opens into its members, or what it
//! points to.

use std::collections::BTreeSet;

use dioxus::prelude::*;
use ksee_core::view::{Frame, Line, Object};

use crate::machine;
use crate::state::Ui;

/// The stack at the cursor (its line's record, the nth line it runs), and
/// that line.
pub fn frames() -> crate::state::Shared<(Vec<Frame>, Option<Line>)> {
    use_context::<crate::state::Derived>().stack.cloned()
}

/// The frame shown: the one chosen, else the innermost.
pub fn shown(ui: Ui, frames: &[Frame]) -> Option<Frame> {
    frames.get((ui.frame)().unwrap_or(0)).or(frames.first()).cloned()
}

/// The next line from `k` in direction `dir`, of those at most as deep as
/// line `k` (`over`), shallower (`out`), or any (into).
fn step(lines: &[Line], k: usize, dir: isize, how: How) -> Option<usize> {
    let here = lines.get(k).map(|l| (l.depth, l.inl))?;
    let fits = |l: &Line| match how {
        How::Into => true,
        How::Over => (l.depth, l.inl) <= here,
        How::Out => (l.depth, l.inl) < here,
    };
    let mut j = k as isize + dir;
    while j >= 0 && (j as usize) < lines.len() {
        if fits(&lines[j as usize]) {
            return Some(j as usize);
        }
        j += dir;
    }
    None
}

#[derive(Clone, Copy, PartialEq)]
enum How {
    Into,
    Over,
    Out,
}

#[component]
pub fn Transport() -> Element {
    let ui = use_context::<Ui>();
    let lines = ui.lines();
    let k = (ui.cursor)();
    let here = lines.get(k);
    let place = here.map(|l| format!("{} {}:{}", l.inlined.as_deref().unwrap_or(&l.func), l.file, l.line)).unwrap_or_default();
    let button = |label: &'static str, title: &'static str, dir: isize, how: How| {
        let lines = lines.clone();
        let to = step(&lines, k, dir, how);
        rsx! { button { title, disabled: to.is_none(), onclick: move |_| if let Some(j) = to { ui.go(j) }, "{label}" } }
    };
    rsx! {
        div { class: "transport",
            {button("◀ out", "back, to the line that called this one", -1, How::Out)}
            {button("◀ over", "back, over the calls this line's function made", -1, How::Over)}
            {button("◀", "the line before", -1, How::Into)}
            {button("▶", "the next line", 1, How::Into)}
            {button("over ▶", "the next line of this function, over the calls it makes", 1, How::Over)}
            {button("out ▶", "on, to the line after this function returns", 1, How::Out)}
            span { class: "mono dim", "{place}" }
            span { class: "dim", " · line {k + 1} of {lines.len()}" }
        }
    }
}

#[component]
pub fn Stack() -> Element {
    let ui = use_context::<Ui>();
    let stack = frames();
    let (frames, line) = (&stack.0 .0, &stack.0 .1);
    let chosen = (ui.frame)().unwrap_or(0);
    let i = line.as_ref().map_or(0, |l| l.i);
    rsx! {
        div { class: "frames",
            for (k , f) in frames.iter().cloned().enumerate() {
                div { class: if k == chosen { "frame on" } else { "frame" },
                    div { class: "fhead", onclick: move |_| { let mut ui = ui; ui.frame.set(Some(k)); },
                        b { class: "mono", "{f.func}" }
                        if let Some(s) = &f.syscall { span { class: "dim", " · {s}" } }
                        if f.inlined { span { class: "dim", " · inlined" } }
                        span { class: "dim mono", " {f.file.clone().unwrap_or_default()}:{f.line.unwrap_or(0)}" }
                    }
                    table { class: "vals",
                        for o in f.args.clone().unwrap_or_default().into_iter().chain(f.locals.clone().unwrap_or_default()) {
                            Value { o: o.clone(), path: format!("{k}.{}", o.name), depth: 0, i }
                        }
                    }
                }
            }
        }
    }
}

/// A value's row, and, opened, its members' (read as memory was at record `i`).
#[component]
fn Value(o: Object, path: String, depth: usize, i: usize) -> Element {
    let mut open = use_context::<Signal<BTreeSet<String>>>();
    let is_open = open.read().contains(&path);
    let can = o.children.is_some() || o.open.is_some();
    let kids = if is_open {
        o.children.clone().or_else(|| o.open.and_then(|t| machine().at_moment(i, |v, mem| v.tables.types.object(mem, &o.name, t.addr, t.ty, 1).children)))
    } else {
        None
    };
    let p = path.clone();
    let value = o.value.clone().unwrap_or_else(|| "{…}".into());
    rsx! {
        tr { class: if o.none.is_some() { "none" } else { "" },
            td { class: "mono", style: "padding-left: {depth * 14 + 4}px",
                if can { a { class: "tw", onclick: move |_| { let mut s = open.write(); if !s.remove(&p) { s.insert(p.clone()); } }, if is_open { "▾ " } else { "▸ " } } }
                "{o.name}"
            }
            td { class: "mono val", "{value}" }
            td { class: "dim mono", "{o.ty}" }
        }
        for c in kids.unwrap_or_default() {
            Value { o: c.clone(), path: format!("{path}.{}", c.name), depth: depth + 1, i }
        }
    }
}
