//! The timeline. On top, an overview of the whole recording: which task
//! ran when, where each syscall starts, the window shown below, the cursor.
//! Below, the window as a stack chart: columns of moments, each its task's
//! stack, the outermost at the bottom; a bar a call (or code inlined into
//! one) over the columns it is on, colored by the kernel's part it is in,
//! the calls at the cursor darker; the task strip under it; then the
//! transport. The wheel zooms, a drag pans; hover a bar: the moment shown
//! everywhere, and what the call was; click it: the cursor there. Find a
//! function by any part of its name.

use dioxus::prelude::*;
use ksee_core::view::{Chart, Move, TaskRef};

use crate::machine;
use crate::state::{Derived, Peek, Shared, Ui};

/// The overview's buckets.
const BUCKETS: usize = 240;
/// A row's height (px): as tall as fits the chart's room, 11 to 15 (deeper,
/// the chart scrolls).
const ROW: f64 = 15.0;
const MIN_ROW: f64 = 11.0;

/// A task's hue, by its pid (the strip, the overview, the program's lanes).
pub fn hue(pid: i32) -> i32 {
    pid * 137 % 360
}

/// A task's color: the idle task's (0) pale.
fn task_color(pid: i32) -> String {
    format!("hsl({} 50% {}%)", hue(pid), if pid == 0 { 85 } else { 62 })
}

/// The color of a call in file `file`: a hue for each of the kernel's
/// parts (arch::AREAS) in order, the last (arch), and none, gray; `on` (at
/// the cursor), darker.
fn color(file: Option<&str>, on: bool) -> String {
    const HUES: [i32; 7] = [210, 135, 28, 48, 280, 175, 330];
    let file = file.unwrap_or("");
    let l = if on { 50 } else { 74 };
    match ksee_core::arch::AREAS.iter().position(|(p, _)| file.starts_with(p)).and_then(|k| HUES.get(k)) {
        Some(h) => format!("hsl({h} {}% {l}%)", if on { 65 } else { 55 }),
        None => format!("hsl(0 0% {}%)", l + 5),
    }
}

/// A task as the page names it: comm(pid).
pub fn named(t: &TaskRef) -> String {
    format!("{}({})", t.comm.clone().unwrap_or_default(), t.pid.unwrap_or(0))
}

/// What the tip over a bar says: lines, where.
#[derive(Clone, PartialEq)]
struct Tip {
    x: f64,
    y: f64,
    lines: Vec<(String, &'static str)>,
}

#[component]
pub fn Timeline() -> Element {
    let mut ui = use_context::<Ui>();
    let tip = use_signal(|| None::<Tip>);
    let title = ui.syscall().map(|s| format!("{} by {}", s.name, named(&s.task))).unwrap_or_default();
    rsx! {
        section { class: "history",
            Overview {}
            div { class: "flamehead",
                span { class: "dim", "{title}"
                    if ui.syscall().is_some() { " · " a { title: "the window at the call selected", onclick: move |_| zoom_to(ui), "zoom" } }
                    if (ui.win)().is_some() { " · " a { title: "the whole recording", onclick: move |_| ui.win.set(None), "whole" } }
                }
                Find {}
                span { class: "legend",
                    for (p , n) in ksee_core::arch::AREAS.iter() { span { i { style: "background: {color(Some(p), false)}" } "{n}" } }
                }
            }
            div { class: "flamescroll", Bars { tip } }
            Strip {}
            Transport {}
        }
        if let Some(t) = tip() {
            div { class: "tip", style: "left: {t.x + 14.0}px; top: {t.y + 16.0}px",
                for (l , c) in t.lines { div { class: c, "{l}" } }
            }
        }
    }
}

/// The window at the call selected, with a little room either side.
fn zoom_to(mut ui: Ui) {
    let Some(s) = ui.syscall() else { return };
    let m = machine();
    let w = m.view(|v| {
        let (a, b) = (v.rec.step_of(s.first), v.rec.step_of(s.last.unwrap_or(v.rec.len().saturating_sub(1))));
        let p = ((b - a) / 50).max(4);
        (v.rec.record_of(a.saturating_sub(p)), v.rec.record_of((b + p).min(v.rec.steps().saturating_sub(1))))
    });
    ui.win.set(Some(w));
}

/// The whole recording, in buckets of steps: who ran in each, each
/// syscall's start, the window and the cursor. A click: the window there,
/// as wide as it is.
#[component]
fn Overview() -> Element {
    let ui = use_context::<Ui>();
    let d = use_context::<Derived>();
    (ui.version)();
    let mut width = use_signal(|| 1000.0f64);
    let m = machine();
    let cursor = d.moment.cloned().map(|(i, _)| i);
    let (n, tasks, starts, (wa, wb), at) = m.view(|v| {
        let n = v.rec.steps().max(1);
        let tasks: Vec<i32> = (0..BUCKETS).map(|k| {
            let i = v.rec.record_of(k * n / BUCKETS);
            v.rec.tasks.get(i).and_then(|&t| v.task(t, i).pid).unwrap_or(0)
        }).collect();
        let starts: Vec<usize> = v.rec.syscalls.iter().map(|s| v.rec.step_of(s.first)).collect();
        let (a, b) = (d.window)();
        (n, tasks, starts, (v.rec.step_of(a), v.rec.step_of(b)), cursor.map(|i| v.rec.step_of(i)))
    });
    let pct = |s: usize| s as f64 * 100.0 / n as f64;
    rsx! {
        div { class: "overview", title: "the whole recording: click to move the window there",
            onresize: move |e| if let Ok(s) = e.get_content_box_size() { width.set(s.width.max(1.0)) },
            onclick: move |e| {
                let mut ui = ui;
                let x = e.element_coordinates().x / width();
                let (s, half) = ((x * n as f64) as usize, (wb - wa) / 2);
                let m = machine();
                ui.win.set(Some(m.view(|v| (v.rec.record_of(s.saturating_sub(half)), v.rec.record_of((s + half).min(n - 1))))));
            },
            for (k , pid) in tasks.into_iter().enumerate() {
                div { class: "b", style: "left: {k as f64 * 100.0 / BUCKETS as f64}%; width: {100.0 / BUCKETS as f64 + 0.1}%; background: {task_color(pid)}" }
            }
            for s in starts { div { class: "start", style: "left: {pct(s)}%" } }
            div { class: "win", style: "left: {pct(wa)}%; width: {pct(wb.saturating_sub(wa)).max(0.3)}%" }
            if let Some(s) = at { div { class: "at", style: "left: {pct(s)}%" } }
        }
    }
}

/// A pan under way: where it began (x), and the window then (steps).
#[derive(Clone, Copy)]
struct Pan {
    x: f64,
    a: usize,
    b: usize,
    moved: bool,
}

#[component]
fn Bars(tip: Signal<Option<Tip>>) -> Element {
    let ui = use_context::<Ui>();
    let d = use_context::<Derived>();
    let chart = d.chart.cloned();
    let mut pan = use_signal(|| None::<Pan>);
    let mut width = use_signal(|| 1000.0f64);
    // (The pointer's x over the chart (px), while it is over a bar: the line is there.)
    let mut hover = use_signal(|| None::<f64>);
    let n = chart.cols.len().max(1);
    let depth = chart.cols.iter().map(|c| c.frames.len()).max().unwrap_or(0);
    // (The rows thinner to fit the chart's room; deeper, it scrolls.)
    let room = use_context::<Signal<crate::layout::Sizes>>()().chart;
    let row = (room / depth.max(1) as f64).clamp(MIN_ROW, ROW).floor();
    // (Each row's runs: a bar over the columns it is on, in a row.)
    let mut runs = Vec::new();
    for dd in 0..depth {
        let mut k = 0;
        while k < n {
            let Some(&bar) = chart.cols.get(k).and_then(|c| c.frames.get(dd)) else { k += 1; continue };
            let mut e = k + 1;
            while chart.cols.get(e).and_then(|c| c.frames.get(dd)) == Some(&bar) {
                e += 1;
            }
            runs.push((dd, k, e, bar as usize));
            k = e;
        }
    }
    let cursor = d.moment.cloned().map(|(i, _)| i);
    let (wa, wb) = (d.window)();
    // (Where the cursor is, across the window (%); the calls on its stack.)
    let now = cursor.filter(|&i| i >= wa && i <= wb).map(|i| machine().view(|v| {
        let (sa, sb, s) = (v.rec.step_of(wa), v.rec.step_of(wb), v.rec.step_of(i));
        (s - sa) as f64 * 100.0 / (sb - sa + 1) as f64
    }));
    let calls: Vec<(usize, bool)> = d.stack.read().iter().map(|f| (f.first, f.inlined)).collect();
    let find = (ui.find)().to_lowercase();
    // (The bar outlined: the frame shown's call.)
    let lit = d.stack.read().get(d.shown.cloned()).map(|f| (f.first, f.inlined));
    let pct = |k: usize| k as f64 * 100.0 / n as f64;
    let px = width() / n as f64;
    // (Zoom: the window narrowed or widened around the pointer's step.)
    let zoom = move |x: f64, by: f64| {
        let mut ui = ui;
        let (a, b) = (d.window)();
        let m = machine();
        let (sa, sb, total) = m.view(|v| (v.rec.step_of(a), v.rec.step_of(b), v.rec.steps().max(1)));
        let span = (sb - sa).max(1) as f64;
        let at = sa as f64 + span * (x / width()).clamp(0.0, 1.0);
        let new = (span * by).clamp(20.0, total as f64);
        let na = (at - (at - sa as f64) * new / span).max(0.0) as usize;
        let nb = ((na as f64 + new) as usize).min(total - 1);
        ui.win.set(Some(m.view(|v| (v.rec.record_of(na), v.rec.record_of(nb)))));
    };
    rsx! {
        div { class: "chart", style: "height: {(depth as f64 * row).max(room)}px",
            onresize: move |e| if let Ok(s) = e.get_content_box_size() { width.set(s.width.max(1.0)) },
            onwheel: move |e| {
                e.prevent_default();
                let dy = e.delta().strip_units().y;
                zoom(e.element_coordinates().x, if dy > 0.0 { 1.25 } else { 0.8 });
            },
            onpointerdown: move |e| {
                tip.set(None);
                let (a, b) = (d.window)();
                let (sa, sb) = machine().view(|v| (v.rec.step_of(a), v.rec.step_of(b)));
                pan.set(Some(Pan { x: e.client_coordinates().x, a: sa, b: sb, moved: false }));
            },
            onpointermove: move |e| {
                let Some(mut p) = pan() else { return };
                let dx = e.client_coordinates().x - p.x;
                if dx.abs() < 4.0 && !p.moved { return }
                p.moved = true;
                pan.set(Some(p));
                let m = machine();
                let total = m.view(|v| v.rec.steps().max(1));
                let shift = (dx / width() * (p.b - p.a) as f64) as isize;
                let na = (p.a as isize - shift).clamp(0, (total - 1 - (p.b - p.a)) as isize) as usize;
                let mut ui = ui;
                ui.win.set(Some(m.view(|v| (v.rec.record_of(na), v.rec.record_of(na + (p.b - p.a))))));
            },
            onpointerup: move |_| pan.set(None),
            onmouseleave: move |_| { let mut ui = ui; ui.peek.set(None); tip.set(None); pan.set(None); hover.set(None); },
            for (dd , a , e , b) in runs {
                {
                    let bar = &chart.bars[b];
                    let (first, inlined) = (bar.first, bar.inlined);
                    // (The moments of the columns it is over, and their stacks' depths:
                    // the pointer's column is the moment shown, and clicked.)
                    let cols: std::rc::Rc<[(usize, usize)]> = chart.cols[a..e].iter().map(|c| (c.i, c.frames.len())).collect();
                    let at = move |x: f64| cols[((x / px).max(0.0) as usize).min(cols.len() - 1)];
                    let (enter, click) = (at.clone(), at.clone());
                    let on = calls.contains(&(first, inlined));
                    let found = !find.is_empty() && bar.name.to_lowercase().contains(&find);
                    let bg = if found { "var(--found)".to_string() } else { color(bar.file.as_deref(), on) };
                    let class = format!("bar{}{}{}", if on { " on" } else { "" }, if found { " found" } else { "" }, if lit == Some((first, inlined)) { " lit" } else { "" });
                    let (name, label) = (bar.name.clone(), bar.label.clone());
                    let wide = (e - a) as f64 * px >= 24.0;
                    rsx! {
                        div { class,
                            style: "left: {pct(a)}%; width: {pct(e - a)}%; bottom: {dd as f64 * row}px; height: {row - 1.0}px; line-height: {row - 1.0}px; background: {bg}",
                            onmouseenter: move |ev| {
                                let mut ui = ui;
                                ui.peek.set(Some(Peek { i: enter(ev.element_coordinates().x).0, first, inlined }));
                                hover.set(Some(a as f64 * px + ev.element_coordinates().x));
                                let p = ev.client_coordinates();
                                tip.set(Some(Tip { x: p.x, y: p.y, lines: said(first, inlined, &name) }));
                            },
                            onmousemove: move |ev| {
                                let mut ui = ui;
                                let peek = Some(Peek { i: at(ev.element_coordinates().x).0, first, inlined });
                                if (ui.peek)() != peek { ui.peek.set(peek); }
                                hover.set(Some(a as f64 * px + ev.element_coordinates().x));
                                let p = ev.client_coordinates();
                                if let Some(mut t) = tip() { t.x = p.x; t.y = p.y; tip.set(Some(t)); }
                            },
                            onclick: move |ev| {
                                if pan().is_some_and(|p| p.moved) { return }
                                let mut ui = ui;
                                ui.peek.set(None);
                                let (i, deep) = click(ev.element_coordinates().x);
                                ui.go_record(i, Some(deep - 1 - dd));
                            },
                            if wide { "{label}" }
                        }
                    }
                }
            }
            match hover() {
                Some(x) => rsx! { div { class: "now", style: "left: {x}px" } },
                None => rsx! { if let Some(x) = now { div { class: "now", style: "left: {x}%" } } },
            }
        }
    }
}

/// What a bar's call was: a syscall's entry, the syscall with its
/// arguments and result, and who made it; a function, what it returned and
/// its arguments; code inlined, its function.
fn said(first: usize, inlined: bool, name: &str) -> Vec<(String, &'static str)> {
    if inlined {
        return vec![(format!("{name} (inlined)"), "")];
    }
    let m = machine();
    m.at_moment(first, |v, mem| {
        let frame = v.call_stack(first, None, mem, first).into_iter().find(|f| !f.inlined);
        let args: Vec<(String, String)> = frame.as_ref().and_then(|f| f.args.clone()).unwrap_or_default().into_iter()
            .map(|a| (a.name, a.value.unwrap_or_else(|| "{…}".into()))).collect();
        let ret = v.rec.calls.call_end(first).and_then(|end| v.at(end)).filter(|a| a.kind == ksee_core::view::RecordKind::Return).and_then(|a| a.value);
        let ret = ret.map(|r| format!(" = {}", v.tables.types.result(r))).unwrap_or_default();
        match v.rec.syscalls.binary_search_by_key(&first, |s| s.first).ok().map(|k| &v.rec.syscalls[k]) {
            Some(s) => {
                let task = v.task(s.task, first);
                let ret = s.ret.map(|r| v.tables.types.result(r as i64)).unwrap_or_else(|| "…".into());
                let shown: Vec<String> = args.iter().map(|(_, x)| x.clone()).collect();
                vec![(format!("{}({}) = {ret}", v.tables.types.syscall_name(s.nr), shown.join(", ")), ""),
                     (format!("a syscall by {}", named(&task)), "dim")]
            }
            None => std::iter::once((format!("{name}{ret}"), "")).chain(args.into_iter().map(|(n, x)| (format!("{n} = {x}"), "f"))).collect(),
        }
    })
}

/// Under the chart: which task ran when, a run of columns each, named as
/// the task was as the run ends (a thread that names itself: as it became).
#[component]
fn Strip() -> Element {
    let d = use_context::<Derived>();
    let chart: Shared<Chart> = d.chart.cloned();
    let n = chart.cols.len().max(1);
    let mut runs: Vec<(usize, usize, u32)> = Vec::new();
    for (k, c) in chart.cols.iter().enumerate() {
        match runs.last_mut() {
            Some(r) if r.2 == c.task => r.1 = k + 1,
            _ => runs.push((k, k + 1, c.task)),
        }
    }
    let m = machine();
    let pct = |k: usize| k as f64 * 100.0 / n as f64;
    rsx! {
        div { class: "strip",
            for (a , e , task) in runs {
                {
                    let t = m.view(|v| v.task(task, chart.cols[e - 1].i));
                    let name = named(&t);
                    rsx! { div { class: "run", title: "{name}", style: "left: {pct(a)}%; width: {pct(e - a)}%; background: {task_color(t.pid.unwrap_or(0))}", "{name}" } }
                }
            }
        }
    }
}

/// Under the strip: steps through the call's lines (◀ ▶; Shift: over the
/// calls, Alt: out of the function, as ← → are), the frame up or down the
/// Stack (▲ ▼, as ↑ ↓), and where the cursor is.
#[component]
fn Transport() -> Element {
    let ui = use_context::<Ui>();
    let lines = ui.lines();
    let here = lines.get((ui.cursor)());
    let s = ui.syscall();
    let task = s.as_ref().map(|s| named(&s.task)).unwrap_or_default();
    let place = here.map(|l| format!("{} {}:{}", l.inlined.as_deref().unwrap_or(&l.func), l.file, l.line)).unwrap_or_default();
    let call = s.as_ref().map(|s| format!(" · in {}", s.name)).unwrap_or_default();
    // (A step that lands after a call: what it returned.)
    let returned = here.and_then(|l| l.called.as_ref()).filter(|c| c.at <= here.map_or(0, |l| l.at))
        .map(|c| format!(" · {} returned {}", c.func, machine().result(c.value))).unwrap_or_default();
    let how = |e: &MouseEvent| if e.modifiers().alt() { Move::Out } else if e.modifiers().shift() { Move::Over } else { Move::Into };
    rsx! {
        div { class: "transport",
            button { title: "the line before (←); Shift: back over the calls; Alt: out, to where the function was called", onclick: move |e| crate::stack::walk(ui, -1, how(&e)), "◀" }
            button { title: "the next line (→); Shift: over the calls; Alt: out, past the function's return", onclick: move |e| crate::stack::walk(ui, 1, how(&e)), "▶" }
            button { title: "the frame up the Stack, the callee (↑)", onclick: move |_| crate::stack::frame(ui, -1), "▲" }
            button { title: "the frame down the Stack, the caller (↓)", onclick: move |_| crate::stack::frame(ui, 1), "▼" }
            span { class: "where mono dim", "{task} · {place}{call}{returned}" }
        }
    }
}

/// Find a function by any part of its name: its bars lit, its calls in the
/// recording counted; Enter, the cursor at its next call (Shift: the one
/// before), round to the start.
#[component]
fn Find() -> Element {
    let mut ui = use_context::<Ui>();
    let w = (ui.find)().to_lowercase();
    let m = machine();
    let calls: Vec<usize> = if w.is_empty() { Vec::new() } else {
        m.view(|v| v.rec.calls(v.tables, |n| n.to_lowercase().contains(&w), 0, usize::MAX, None).collect())
    };
    let count = calls.len();
    rsx! {
        input { class: "find mono", placeholder: "find a function…", value: "{ui.find}", spellcheck: false,
            title: "any part of a function's name (its bars colored as you type): Enter, its next call after the cursor; Shift+Enter, the one before; Esc clears",
            oninput: move |e| ui.find.set(e.value()),
            onkeydown: move |e| {
                if e.key() == Key::Escape { ui.find.set(String::new()); return }
                if e.key() != Key::Enter || calls.is_empty() { return }
                let here = consume_context::<Derived>().moment.cloned().map_or(0, |(i, _)| i);
                let to = if e.modifiers().shift() {
                    calls.iter().rev().find(|&&c| c < here).or(calls.last())
                } else {
                    calls.iter().find(|&&c| c > here).or(calls.first())
                };
                if let Some(&i) = to {
                    ui.go_record(i, None);
                    // (Its window, around it, if outside.)
                    let (a, b) = consume_context::<Derived>().window.cloned();
                    if i < a || i > b {
                        let half = (b - a) / 2;
                        ui.win.set(Some((i.saturating_sub(half), i + half)));
                    }
                }
            } }
        if !w.is_empty() { span { class: "dim", if count == 0 { "none in the recording" } else { "{count} calls" } } }
    }
}
