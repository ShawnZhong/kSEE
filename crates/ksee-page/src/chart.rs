//! The timeline. On top, an overview of the whole recording: which task
//! ran when, where each syscall starts, the window shown below, the cursor.
//! Below, the window as a stack chart: columns of moments, each its task's
//! stack, the outermost at the bottom; a bar a call (or code inlined into
//! one) over the columns it is on, colored by the kernel's part it is in,
//! the calls at the cursor darker; the task strip under it; then the
//! transport. The wheel zooms, a drag pans; hover a bar: the moment shown
//! everywhere but the cursor's line, and what the call was; click it: the
//! cursor there. Find a function by any part of its name.

use dioxus::prelude::*;
use ksee_core::view::Chart;

use crate::machine;
use crate::state::{Derived, Shared, Ui};

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

/// What the tip over a bar says: lines, where.
#[derive(Clone, PartialEq)]
struct Tip {
    x: f64,
    y: f64,
    /// (The bar's call it says: its first record and function.)
    call: (usize, String),
    lines: Vec<(String, &'static str)>,
}

#[component]
pub fn Timeline() -> Element {
    let mut ui = use_context::<Ui>();
    let tip = use_signal(|| None::<Tip>);
    let title = ui.syscall().map(|s| format!("{} by {}", s.name, s.task.named())).unwrap_or_default();
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
                    for (p , n) in ksee_core::arch::AREAS.iter() { span { i { style: "background: {ksee_core::arch::color(Some(p), false)}" } "{n}" } }
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
    let cursor = d.cursor.cloned();
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
    // (Latched through pointer-up: a click fires after it, when pan is
    // already none — so a drag never ends in a click.)
    let mut dragged = use_signal(|| false);
    let mut width = use_signal(|| 1000.0f64);
    let n = chart.cols.len().max(1);
    let depth = chart.cols.iter().map(|c| c.frames.len()).max().unwrap_or(0);
    // (The rows thinner to fit the chart's room; deeper, it scrolls.)
    let room = use_context::<Signal<crate::layout::Sizes>>()().chart;
    let row = (room / depth.max(1) as f64).clamp(MIN_ROW, ROW).floor();
    let runs = chart.bar_runs();
    let cursor = d.cursor.cloned();
    let (wa, wb) = (d.window)();
    // (Where a record is, across the window (%): the cursor's, solid; the
    // moment under the pointer's, faint; the calls on its stack.)
    let x = |i: usize| (i >= wa && i <= wb).then(|| machine().view(|v| {
        let (sa, sb, s) = (v.rec.step_of(wa), v.rec.step_of(wb), v.rec.step_of(i));
        (s - sa) as f64 * 100.0 / (sb - sa + 1) as f64
    }));
    let now = cursor.and_then(x);
    let peeked = ui.peek.read().as_ref().and(d.moment.cloned()).and_then(|(i, _)| x(i));
    let calls: Vec<(usize, String)> = d.stack.read().iter().map(|f| (f.first, f.func.clone())).collect();
    let find = (ui.find)().to_lowercase();
    // (The bar outlined: the one under the pointer, else the frame shown's call.)
    let lit = tip().map(|t| t.call).or_else(|| d.stack.read().get(d.frame.cloned()).map(|f| (f.first, f.func.clone())));
    let height = (depth as f64 * row).max(room);
    // (What is under the pointer, at x, y in the chart: its column, and the bar there, if
    // one: its depth from the outermost.)
    let hit = move |chart: &Chart, x: f64, y: f64| -> Option<(usize, Option<usize>)> {
        let k = ((x / width() * chart.cols.len() as f64).max(0.0) as usize).min(chart.cols.len().checked_sub(1)?);
        let dd = ((height - y) / row).floor();
        Some((k, (dd >= 0.0 && (dd as usize) < chart.cols[k].frames.len()).then_some(dd as usize)))
    };
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
        div { class: "chart", style: "height: {height}px",
            onresize: move |e| if let Ok(s) = e.get_content_box_size() { width.set(s.width.max(1.0)) },
            onwheel: move |e| {
                e.prevent_default();
                let dy = e.delta().strip_units().y;
                zoom(e.element_coordinates().x, if dy > 0.0 { 1.25 } else { 0.8 });
            },
            onpointerdown: move |e| {
                tip.set(None);
                dragged.set(false);
                let (a, b) = (d.window)();
                let (sa, sb) = machine().view(|v| (v.rec.step_of(a), v.rec.step_of(b)));
                pan.set(Some(Pan { x: e.client_coordinates().x, a: sa, b: sb, moved: false }));
            },
            onpointermove: {
                let chart = chart.clone();
                move |e: PointerEvent| {
                    let mut ui = ui;
                    // (A drag: the window panned.)
                    if let Some(mut p) = pan() {
                        let dx = e.client_coordinates().x - p.x;
                        if dx.abs() >= 4.0 || p.moved {
                            p.moved = true;
                            dragged.set(true);
                            pan.set(Some(p));
                            let m = machine();
                            let total = m.view(|v| v.rec.steps().max(1));
                            let shift = (dx / width() * (p.b - p.a) as f64) as isize;
                            // (Saturated: the window can be the whole recording.)
                            let max = (total - 1).saturating_sub(p.b - p.a);
                            let na = (p.a as isize - shift).clamp(0, max as isize) as usize;
                            ui.win.set(Some(m.view(|v| (v.rec.record_of(na), v.rec.record_of(na + (p.b - p.a))))));
                            return;
                        }
                    }
                    // (Else the moment under the pointer shown everywhere but the line: over a bar, what its call was.)
                    let at = e.element_coordinates();
                    let Some((k, dd)) = hit(&chart, at.x, at.y) else { return };
                    let call = dd.map(|dd| &chart.bars[chart.cols[k].frames[dd] as usize]).map(|b| (b.first, b.name.clone()));
                    let peek = ui.moment_at(chart.cols[k].i, call);
                    if *ui.peek.peek() != peek { ui.peek.set(peek); }
                    let p = e.client_coordinates();
                    match dd.map(|dd| &chart.bars[chart.cols[k].frames[dd] as usize]) {
                        None => if tip().is_some() { tip.set(None) },
                        Some(b) => match tip() {
                            Some(t) if t.call.0 == b.first && t.call.1 == b.name => tip.set(Some(Tip { x: p.x, y: p.y, ..t })),
                            _ => tip.set(Some(Tip { x: p.x, y: p.y, call: (b.first, b.name.clone()), lines: said(b.first, &b.name, b.inlined) })),
                        },
                    }
                }
            },
            onpointerup: move |_| pan.set(None),
            // (A click, not a drag: the moment previewed committed, as it is.)
            onclick: {
                let chart = chart.clone();
                move |e: MouseEvent| {
                    if dragged() { dragged.set(false); return }
                    let mut ui = ui;
                    let at = e.element_coordinates();
                    let peek = ui.peek.peek().clone().or_else(|| {
                        let (k, dd) = hit(&chart, at.x, at.y)?;
                        let call = dd.map(|dd| &chart.bars[chart.cols[k].frames[dd] as usize]).map(|b| (b.first, b.name.clone()));
                        ui.moment_at(chart.cols[k].i, call)
                    });
                    if peek.is_some() { ui.at.set(peek); }
                    ui.peek.set(None);
                }
            },
            onmouseleave: move |_| { let mut ui = ui; ui.peek.set(None); tip.set(None); pan.set(None); dragged.set(false); },
            for (dd , a , e , b) in runs {
                {
                    let bar = &chart.bars[b as usize];
                    let id = (bar.first, bar.name.clone());
                    let on = calls.contains(&id);
                    let found = !find.is_empty() && bar.name.to_lowercase().contains(&find);
                    let bg = if found { "var(--found)".to_string() } else { ksee_core::arch::color(bar.file.as_deref(), on) };
                    let class = format!("bar{}{}{}", if on { " on" } else { "" }, if found { " found" } else { "" }, if lit.as_ref().is_some_and(|l| l.0 == id.0 && l.1 == id.1) { " lit" } else { "" });
                    let label = bar.label.clone();
                    let wide = (e - a) as f64 * px >= 24.0;
                    rsx! {
                        div { class,
                            style: "left: {pct(a)}%; width: {pct(e - a)}%; bottom: {dd as f64 * row}px; height: {row - 1.0}px; line-height: {row - 1.0}px; background: {bg}",
                            if wide { "{label}" }
                        }
                    }
                }
            }
            if let Some(x) = peeked { div { class: "now peek", style: "left: {x}%" } }
            if let Some(x) = now { div { class: "now", style: "left: {x}%" } }
        }
    }
}

/// What a bar's call was: a syscall's entry, the syscall with its
/// arguments and result, and who made it; a function, what it returned and
/// its arguments; code inlined, its function.
fn said(first: usize, name: &str, inlined: bool) -> Vec<(String, &'static str)> {
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
                     (format!("a syscall by {}", task.named()), "dim")]
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
    let runs = chart.task_runs();
    let m = machine();
    let pct = |k: usize| k as f64 * 100.0 / n as f64;
    rsx! {
        div { class: "strip",
            for (a , e , task) in runs {
                {
                    let t = m.view(|v| v.task(task, chart.cols[e - 1].i));
                    let name = t.named();
                    rsx! { div { class: "run", title: "{name}", style: "left: {pct(a)}%; width: {pct(e - a)}%; background: {task_color(t.pid.unwrap_or(0))}", "{name}" } }
                }
            }
        }
    }
}

/// Under the strip: steps through the call's lines (◀ ▶; Shift: over the
/// calls, as ← → are), the frame up or down the
/// Stack (▲ ▼, as ↑ ↓), and where the cursor is.
#[component]
fn Transport() -> Element {
    let ui = use_context::<Ui>();
    let d = use_context::<Derived>();
    let lines = ui.lines();
    let here = use_context::<Derived>().shown.read().as_ref().and_then(|m| lines.get(m.line));
    let s = ui.syscall();
    let task = s.as_ref().map(|s| s.task.named()).unwrap_or_default();
    let place = here.map(|l| format!("{} {}:{}", l.inlined.as_deref().unwrap_or(&l.func), l.file, l.line)).unwrap_or_default();
    // (A line that runs more than once in this call: which turn the cursor is on.)
    let turn = here.filter(|l| l.run > 1).map(|l| format!(" · turn {}", l.run)).unwrap_or_default();
    // (A step that lands after a call: what it returned.)
    let returned = here.and_then(|l| l.called.as_ref()).filter(|c| c.at <= here.map_or(0, |l| l.at))
        .map(|c| format!(" · {} returned {}", c.func, machine().result(c.value))).unwrap_or_default();
    rsx! {
        div { class: "transport",
            button { title: "the line before (←); Shift: back over the calls", onclick: move |e| crate::stack::walk(ui, -1, e.modifiers().shift()), "◀" }
            button { title: "the next line (→); Shift: over the calls", onclick: move |e| crate::stack::walk(ui, 1, e.modifiers().shift()), "▶" }
            button { title: "the frame up the Stack, the callee (↑)", onclick: move |_| crate::stack::frame(ui, d, -1), "▲" }
            button { title: "the frame down the Stack, the caller (↓)", onclick: move |_| crate::stack::frame(ui, d, 1), "▼" }
            span { class: "where mono dim", "{task} · {place}{turn}{returned}" }
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
