//! The timeline: an overview of the whole recording (which task ran when,
//! the window marked), and the window as a stack chart: columns of moments,
//! each its task's stack, the outermost at the bottom; a bar a call (or code
//! inlined into one) over the columns it is on, colored by the kernel's part
//! it is in; the task strip under it. The wheel zooms, a drag pans; hover a
//! bar: the moment shown everywhere, and what the call was; click it: the
//! cursor there. Find a function by any part of its name.

use dioxus::prelude::*;
use ksee_core::view::Chart;

use crate::machine;
use crate::state::{Derived, Peek, Shared, Ui};

/// Columns, the overview's buckets, and a row's height (px).
pub const COLUMNS: usize = 300;
const BUCKETS: usize = 240;
/// A row's height (px): as tall as fits the chart's room, 9 to 17.
const ROW: f64 = 17.0;
const MIN_ROW: f64 = 9.0;

/// Where file `file` is in the kernel (arch::AREAS): its color's class.
pub fn area(file: Option<&str>) -> &'static str {
    let file = file.unwrap_or("");
    ksee_core::arch::AREAS.iter().find(|(p, _)| file.starts_with(p)).map_or("other", |(_, n)| n)
}

/// A task's color, by its pid.
fn hue(pid: i32) -> String {
    format!("hsl({} 55% 72%)", pid * 47 % 360)
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
    let ui = use_context::<Ui>();
    let d = use_context::<Derived>();
    let tip = use_signal(|| None::<Tip>);
    let (a, b) = (d.window)();
    let m = machine();
    let steps = m.view(|v| (v.rec.step_of(a), v.rec.step_of(b)));
    rsx! {
        div { class: "chartbox",
            div { class: "head",
                Find {}
                if (ui.win)().is_some() { a { onclick: move |_| { let mut ui = ui; ui.win.set(None); }, title: "the window at the call selected", "zoom" } }
                a { onclick: move |_| { let mut ui = ui; ui.win.set(Some((0, machine().records().saturating_sub(1)))); }, title: "the whole recording", "whole" }
                span { class: "dim", "steps {steps.0}–{steps.1}" }
                span { class: "legend",
                    for (_ , n) in ksee_core::arch::AREAS.iter() { span { i { class: "{n}" } "{n}" } }
                }
            }
            Overview {}
            Bars { tip }
            Strip {}
        }
        if let Some(t) = tip() {
            div { class: "tip", style: "left: {t.x + 14.0}px; top: {t.y + 14.0}px",
                for (l , c) in t.lines { div { class: c, "{l}" } }
            }
        }
    }
}

/// The whole recording, in buckets of steps: who ran in each; the window,
/// marked. A click: the window there, as wide as it is.
#[component]
fn Overview() -> Element {
    let ui = use_context::<Ui>();
    let d = use_context::<Derived>();
    (ui.version)();
    let m = machine();
    let (n, tasks, (wa, wb)) = m.view(|v| {
        let n = v.rec.steps().max(1);
        let tasks: Vec<Option<i32>> = (0..BUCKETS).map(|k| {
            let i = v.rec.record_of(k * n / BUCKETS);
            v.rec.tasks.get(i).and_then(|&t| v.task(t, i).pid)
        }).collect();
        let (a, b) = (d.window)();
        (n, tasks, (v.rec.step_of(a), v.rec.step_of(b)))
    });
    let pct = |s: usize| s as f64 * 100.0 / n as f64;
    rsx! {
        div { class: "overview", title: "the whole recording: click to move the window there",
            for (k , pid) in tasks.into_iter().enumerate() {
                div { class: "b", style: "left: {k as f64 * 100.0 / BUCKETS as f64}%; width: {100.0 / BUCKETS as f64}%; background: {hue(pid.unwrap_or(0))}",
                    onclick: move |_| {
                        let mut ui = ui;
                        let m = machine();
                        let (s, half) = (k * n / BUCKETS, (wb - wa) / 2);
                        let (a, b) = m.view(|v| (v.rec.record_of(s.saturating_sub(half)), v.rec.record_of((s + half).min(n - 1))));
                        ui.win.set(Some((a, b)));
                    } }
            }
            div { class: "win", style: "left: {pct(wa)}%; width: {pct(wb.saturating_sub(wa)).max(0.3)}%" }
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
    let n = chart.cols.len().max(1);
    let depth = chart.cols.iter().map(|c| c.frames.len()).max().unwrap_or(0);
    // (The rows thinner to fit the chart's room, its head, overview and strip aside; deeper, it scrolls.)
    let room = use_context::<Signal<crate::layout::Sizes>>()().chart - 64.0;
    let row = (room / depth.max(1) as f64).clamp(MIN_ROW, ROW).floor();
    let labels = row >= 12.0;
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
    let lines = ui.lines();
    let cursor = lines.get((ui.cursor)()).map(|l| l.i);
    let col = |i: usize| chart.cols.iter().rposition(|c| c.i <= i);
    let now = cursor.and_then(col);
    let find = (ui.find)().to_lowercase();
    // (The bar outlined: the frame shown's call.)
    let lit = d.stack.read().get(d.shown.cloned()).map(|f| (f.first, f.inlined));
    let pct = |k: usize| k as f64 * 100.0 / n as f64;
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
        div { class: "chart", style: "height: {depth as f64 * row}px",
            onmounted: move |e| async move { if let Ok(r) = e.get_client_rect().await { width.set(r.size.width); } },
            onwheel: move |e| {
                e.prevent_default();
                let dy = e.delta().strip_units().y;
                zoom(e.element_coordinates().x, if dy > 0.0 { 1.25 } else { 0.8 });
            },
            onpointerdown: move |e| {
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
            onmouseleave: move |_| { let mut ui = ui; ui.peek.set(None); tip.set(None); pan.set(None); },
            for (dd , a , e , b) in runs {
                {
                    let bar = &chart.bars[b];
                    let (i, deep, first, inlined) = (chart.cols[a].i, chart.cols[a].frames.len(), bar.first, bar.inlined);
                    let class = if !find.is_empty() && bar.name.to_lowercase().contains(&find) { "bar found".to_string() } else { format!("bar {}", area(bar.file.as_deref())) };
                    let class = if lit == Some((first, inlined)) { format!("{class} lit") } else { class };
                    let (name, label) = (bar.name.clone(), bar.label.clone());
                    rsx! {
                        div { class,
                            style: "left: {pct(a)}%; width: {pct(e - a)}%; bottom: {dd as f64 * row}px; height: {row - 1.0}px; line-height: {row - 1.0}px",
                            onmouseenter: move |ev| {
                                let mut ui = ui;
                                ui.peek.set(Some(Peek { i, first, inlined }));
                                let p = ev.client_coordinates();
                                tip.set(Some(Tip { x: p.x, y: p.y, lines: said(first, inlined, &name) }));
                            },
                            onmousemove: move |ev| {
                                let p = ev.client_coordinates();
                                if let Some(mut t) = tip() { t.x = p.x; t.y = p.y; tip.set(Some(t)); }
                            },
                            onclick: move |_| {
                                if pan().is_some_and(|p| p.moved) { return }
                                let mut ui = ui;
                                ui.peek.set(None);
                                ui.go_record(i, Some(deep - 1 - dd));
                            },
                            if labels { "{label}" }
                        }
                    }
                }
            }
            if let Some(k) = now { div { class: "now", style: "left: {pct(k)}%" } }
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
                     (format!("a syscall by {}({})", task.comm.unwrap_or_default(), task.pid.unwrap_or(0)), "dim")]
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
                    let name = format!("{}({})", t.comm.unwrap_or_default(), t.pid.unwrap_or(0));
                    rsx! { div { class: "run", title: "{name}", style: "left: {pct(a)}%; width: {pct(e - a)}%; background: {hue(t.pid.unwrap_or(0))}", "{name}" } }
                }
            }
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
        input { class: "find", placeholder: "find a function…", value: "{ui.find}",
            oninput: move |e| ui.find.set(e.value()),
            onkeydown: move |e| if e.key() == Key::Enter && !calls.is_empty() {
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
        if !w.is_empty() { span { class: "dim", "{count} calls" } }
    }
}
