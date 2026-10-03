//! The timeline. On top, an overview of the whole recording: which task
//! ran when, where each syscall starts, the window shown below, the cursor.
//! Below, the window as a stack chart: columns of moments, each its task's
//! stack, the outermost at the bottom; a bar a call (or code inlined into
//! one) over the columns it is on, colored by the kernel's part it is in,
//! the calls at the cursor darker; the task strip under it; then the
//! transport. The wheel zooms vertically and pans horizontally; a drag pans;
//! everywhere but the cursor's line, and what the call was; click it: the
//! cursor there. Find a function by any part of its name.

use dioxus::prelude::*;
use ksee_core::trace::Record;
use ksee_core::view::{wheel_window, Chart};
use wasm_bindgen::JsCast;

use crate::machine;
use crate::state::{Derived, Shared, Ui};

/// The overview's buckets.
const BUCKETS: usize = 240;
/// A stack row's fixed height (px); deeper charts scroll.
const ROW: f64 = 15.0;
/// How much a vertical wheel turn (a pixel of it) zooms.
const ZOOM: f64 = 0.002;

/// The chart window made by the page's wheel input.
pub(crate) fn wheel_view(window: (usize, usize), x: f64, width: f64, dx: f64, dy: f64, shift: bool) -> (f64, usize) {
    wheel_window(window, x, width, dx, dy, shift, (dy * ZOOM).exp())
}
/// Kernel areas in timeline legend order; unmatched files use gray.
const AREAS: &[(&str, &str)] = &[("fs/", "fs"), ("mm/", "mm"), ("kernel/sched/", "sched"), ("kernel/", "kernel"),
                                  ("drivers/", "drivers"), ("lib/", "lib"), ("include/", "include"), ("arch/", "arch")];

/// A task's hue, by its pid (the strip, the overview, the program's lanes).
pub fn hue(pid: i32) -> i32 {
    pid * 137 % 360
}

/// A task's color: the idle task's (0) pale.
fn task_color(pid: i32) -> String {
    format!("hsl({} 50% {}%)", hue(pid), if pid == 0 { 85 } else { 62 })
}

/// A call's color, by the first kernel area its source file belongs to.
pub(crate) fn call_color(file: Option<&str>, on: bool) -> String {
    const HUES: [i32; 7] = [210, 135, 28, 48, 280, 175, 330];
    let l = if on { 50 } else { 74 };
    match AREAS.iter().position(|(p, _)| file.unwrap_or("").starts_with(p)).and_then(|k| HUES.get(k)) {
        Some(h) => format!("hsl({h} {}% {l}%)", if on { 65 } else { 55 }),
        None => format!("hsl(0 0% {}%)", l + 5),
    }
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
    let ui = use_context::<Ui>();
    let tip = use_signal(|| None::<Tip>);
    // (The cursor stepped out of the window: the window moved to it, as wide.)
    let d = use_context::<Derived>();
    use_effect(move || {
        let Some(at) = d.cursor.cloned() else { return };
        let (a, b) = *d.window.peek();
        if at < a as f64 || at >= (b + 1) as f64 {
            ui.view(at - ((b - a) / 2) as f64, b - a);
        }
    });
    let title = d.syscall.cloned().map(|s| format!("{} by {}", s.name, s.task.named())).unwrap_or_default();
    rsx! {
        section { class: "history",
            Overview {}
            div { class: "flamehead",
                span { class: "dim", "{title}"
                    if let Some(k) = ui.call() { " · " a { title: "the window at the call selected", onclick: move |_| ui.zoom(k), "zoom" } }
                    if (ui.win)().is_some() { " · " a { title: "the whole recording", onclick: move |_| ui.set_window(None), "whole" } }
                }
                Find {}
                span { class: "legend",
                    for (p , n) in AREAS { span { i { style: "background: {call_color(Some(p), false)}" } "{n}" } }
                }
            }
            div { id: "timeline-scroll", class: "flamescroll", Bars { tip } }
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

/// The whole recording, in buckets of steps: who ran in each, each
/// syscall's start, the window and the cursor. A click: the window there,
/// as wide as it is.
#[component]
fn Overview() -> Element {
    let ui = use_context::<Ui>();
    let d = use_context::<Derived>();
    (ui.snapshot)();
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
        (n, tasks, starts, (d.window)(), cursor)
    });
    let pct = |s: usize| s as f64 * 100.0 / n as f64;
    rsx! {
        div { class: "overview", title: "the whole recording: click to move the window there",
            onresize: move |e| if let Ok(s) = e.get_content_box_size() { width.set(s.width.max(1.0)) },
            onclick: move |e| {
                let x = e.element_coordinates().x / width();
                ui.view(x * n as f64 - ((wb - wa) / 2) as f64, wb - wa);
            },
            for (k , pid) in tasks.into_iter().enumerate() {
                div { class: "b", style: "left: {k as f64 * 100.0 / BUCKETS as f64}%; width: {100.0 / BUCKETS as f64 + 0.1}%; background: {task_color(pid)}" }
            }
            for s in starts { div { class: "start", style: "left: {pct(s)}%" } }
            div { class: "win", style: "left: {pct(wa)}%; width: {pct(wb.saturating_sub(wa)).max(0.3)}%" }
            if let Some(s) = at { div { class: "at", style: "left: {s * 100.0 / n as f64}%" } }
        }
    }
}

/// A pan under way: where it began, and the time and stack windows then.
#[derive(Clone, Copy)]
struct Pan {
    x: f64,
    y: f64,
    scroll: i32,
    a: usize,
    b: usize,
}

fn element(id: &str) -> Option<web_sys::HtmlElement> {
    web_sys::window()?.document()?.get_element_by_id(id)?.dyn_into().ok()
}

fn stack_scroll() -> Option<web_sys::HtmlElement> {
    element("timeline-scroll")
}

fn preview_drag(height: f64, dx: f64) {
    if let Some(chart) = element("timeline-chart") {
        let transform = if dx == 0.0 { String::new() } else { format!(" transform: translateX({dx}px);") };
        let _ = chart.set_attribute("style", &format!("height: {height}px;{transform}"));
    }
}

fn drag_moved(dx: f64, dy: f64) -> bool {
    dx.hypot(dy) >= 4.0
}

fn drag_scroll(start: i32, dy: f64, max: i32) -> i32 {
    ((start as f64 - dy).round() as i32).clamp(0, max)
}

fn chart_height(depth: usize, room: f64) -> f64 {
    (depth as f64 * ROW).max(room)
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
    let room = use_context::<Signal<crate::layout::Sizes>>()().chart;
    let row = ROW;
    let runs = chart.bar_runs();
    let (sa, sb) = (d.window)();
    // (Where a step is, across the window (%): the cursor's, solid; the
    // moment under the pointer's, faint; the calls on its stack.)
    let x = |s: f64| (s >= sa as f64 && s < (sb + 1) as f64).then(|| (s - sa as f64) * 100.0 / (sb - sa + 1) as f64);
    let now = d.cursor.cloned().and_then(x);
    let peeked = d.preview.cloned().and_then(x);
    let calls: Vec<(usize, String)> = d.stack.read().iter().map(|f| (f.first, f.func.clone())).collect();
    let find = (ui.find)().to_lowercase();
    // (The bar outlined: the one under the pointer, else the frame shown's call.)
    let lit = tip().map(|t| t.call).or_else(|| d.stack.read().get(d.frame.cloned()).map(|f| (f.first, f.func.clone())));
    let height = chart_height(depth, room);
    let mut bottomed = use_signal(|| false);
    use_effect(move || {
        let _ = d.chart.read();
        if !bottomed() {
            if let Some(scroll) = stack_scroll() {
                let max = scroll.scroll_height() - scroll.client_height();
                if max > 0 {
                    scroll.set_scroll_top(max);
                    bottomed.set(true);
                }
            }
        }
    });
    // (One rule for hover and click:
    // and the moment that column samples.)
    let hit = move |chart: &Chart, x: f64, y: f64| -> Option<(Option<crate::state::Moment>, Option<usize>)> {
        let k = ((x / width() * chart.cols.len() as f64).max(0.0) as usize).min(chart.cols.len().checked_sub(1)?);
        let dd = ((height - y) / row).floor();
        let bar = (dd >= 0.0 && (dd as usize) < chart.cols[k].frames.len()).then(|| chart.cols[k].frames[dd as usize] as usize);
        let frame = bar.map(|b| (chart.bars[b].first, chart.bars[b].name.clone()));
        let shown = d.shown.peek().as_ref().map(|m| m.call);
        Some((ui.moment_at(chart.cols[k].i, Some(chart.cols[k].nth), frame, shown, &d.lines.read()), bar))
    };
    let pct = |k: usize| k as f64 * 100.0 / n as f64;
    let px = width() / n as f64;
    // (The wheel: up and down zoom around the pointer's step, in
    // proportion to how far it turned; left and right (or Shift) pan.)
    let wheel = move |x: f64, dx: f64, dy: f64, shift: bool| {
        let (start, span) = wheel_view((d.window)(), x, width(), dx, dy, shift);
        ui.view(start, span);
    };
    rsx! {
        div { id: "timeline-chart", class: "chart", style: "height: {height}px",
            onresize: move |e| if let Ok(s) = e.get_content_box_size() { width.set(s.width.max(1.0)) },
            onwheel: move |e| {
                e.prevent_default();
                let d = e.delta().strip_units();
                wheel(e.element_coordinates().x, d.x, d.y, e.modifiers().shift());
            },
            onpointerdown: move |e| {
                e.prevent_default();
                tip.set(None);
                ui.set_preview(None);
                dragged.set(false);
                let p = e.client_coordinates();
                let scroll = stack_scroll().map_or(0, |e| e.scroll_top());
                let (a, b) = (d.window)();
                pan.set(Some(Pan { x: p.x, y: p.y, scroll, a, b }));
            },
            onpointermove: {
                let chart = chart.clone();
                move |e: PointerEvent| {
                    let ui = ui;
                    if let Some(p) = pan() {
                        let at = e.client_coordinates();
                        let (dx, dy) = (at.x - p.x, at.y - p.y);
                        if drag_moved(dx, dy) || *dragged.peek() {
                            if !*dragged.peek() {
                                dragged.set(true);
                            }
                            ui.set_preview(None);
                            tip.set(None);
                            preview_drag(height, dx);
                            if let Some(scroll) = stack_scroll() {
                                let max = scroll.scroll_height() - scroll.client_height();
                                scroll.set_scroll_top(drag_scroll(p.scroll, dy, max));
                            }
                            return;
                        }
                    }
                    // (Else the moment under the pointer shown everywhere but the line: over a bar, what its call was.)
                    let at = e.element_coordinates();
                    let Some((peek, bar)) = hit(&chart, at.x, at.y) else { return };
                    ui.set_preview(peek);
                    let p = e.client_coordinates();
                    match bar.map(|b| &chart.bars[b]) {
                        None => if tip().is_some() { tip.set(None) },
                        Some(b) => match tip() {
                            Some(t) if t.call.0 == b.first && t.call.1 == b.name => tip.set(Some(Tip { x: p.x, y: p.y, ..t })),
                            _ => tip.set(Some(Tip { x: p.x, y: p.y, call: (b.first, b.name.clone()), lines: said(b.first, &b.name, b.inlined) })),
                        },
                    }
                }
            },
            onpointerup: move |e| {
                if let Some(p) = pan() {
                    let dx = e.client_coordinates().x - p.x;
                    preview_drag(height, 0.0);
                    pan.set(None);
                    if *dragged.peek() && dx != 0.0 {
                        ui.view(p.a as f64 - dx / width() * (p.b - p.a + 1) as f64, p.b - p.a);
                    }
                }
            },
            // (A click, not a drag: resolve its coordinates afresh, then commit.)
            onclick: {
                let chart = chart.clone();
                move |e: MouseEvent| {
                    if dragged() { dragged.set(false); return }
                    let ui = ui;
                    let at = e.element_coordinates();
                    if let Some(moment) = hit(&chart, at.x, at.y).and_then(|h| h.0) { ui.commit(Some(moment)); }
                }
            },
            onpointercancel: move |_| {
                preview_drag(height, 0.0);
                pan.set(None);
            },
            onmouseleave: move |_| {
                ui.set_preview(None);
                tip.set(None);
                preview_drag(height, 0.0);
                pan.set(None);
            },
            for (dd , a , e , b) in runs {
                {
                    let bar = &chart.bars[b as usize];
                    let id = (bar.first, bar.name.clone());
                    let on = calls.contains(&id);
                    let found = !find.is_empty() && bar.name.to_lowercase().contains(&find);
                    let bg = if found { "var(--found)".to_string() } else { call_color(bar.file.as_deref(), on) };
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
        let frame = v.call_stack(first, None, 0, mem, first).into_iter().find(|f| !f.inlined);
        let args: Vec<(String, String)> = frame.as_ref().and_then(|f| f.args.clone()).unwrap_or_default().into_iter()
            .map(|a| (a.name, a.value.unwrap_or_else(|| "{…}".into()))).collect();
        let ret = v.rec.calls.call_end(first).and_then(|end| match v.rec.records.get(end) { Some(&Record::Return { value, .. }) => Some(value), _ => None });
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
    let here = d.line.cloned();
    let s = d.syscall.cloned();
    let task = s.as_ref().map(|s| s.task.named()).unwrap_or_default();
    let place = here.as_ref().map(|l| format!("{} {}:{}", l.inlined.as_deref().unwrap_or(&l.func), l.file, l.line)).unwrap_or_default();
    // (A line that runs more than once in this call: which turn the cursor is on.)
    let turn = here.as_ref().filter(|l| l.run > 1).map(|l| format!(" · turn {}", l.run)).unwrap_or_default();
    // (A step that lands after a call: what it returned.)
    let returned = here.as_ref().and_then(|l| l.called.as_ref()).filter(|c| c.at <= here.as_ref().map_or(0, |l| l.at))
        .map(|c| format!(" · {} returned {}", c.func, machine().result(c.value))).unwrap_or_default();
    rsx! {
        div { class: "transport",
            button { title: "the line before (←); Shift: back over the calls", onclick: move |e| crate::stack::walk(ui, &machine(), -1, e.modifiers().shift()), "◀" }
            button { title: "the next line (→); Shift: over the calls", onclick: move |e| crate::stack::walk(ui, &machine(), 1, e.modifiers().shift()), "▶" }
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
    let ui = use_context::<Ui>();
    let d = use_context::<Derived>();
    (ui.snapshot)();
    let w = (ui.find)().to_lowercase();
    let m = machine();
    let calls: Vec<usize> = if w.is_empty() { Vec::new() } else {
        m.view(|v| v.rec.calls(v.tables, |n| n.to_lowercase().contains(&w), 0, usize::MAX, None).collect())
    };
    let count = calls.len();
    rsx! {
        input { class: "find mono", placeholder: "find a function…", value: "{ui.find}", spellcheck: false,
            title: "any part of a function's name (its bars colored as you type): Enter, its next call after the cursor; Shift+Enter, the one before; Esc clears",
            oninput: move |e| ui.set_function_filter(e.value()),
            onkeydown: move |e| {
                if e.key() == Key::Escape { ui.set_function_filter(String::new()); return }
                if e.key() != Key::Enter || calls.is_empty() { return }
                let here = d.line.cloned().map_or(0, |l| l.i);
                let to = if e.modifiers().shift() {
                    calls.iter().rev().find(|&&c| c < here).or(calls.last())
                } else {
                    calls.iter().find(|&&c| c > here).or(calls.first())
                };
                if let Some(&i) = to {
                    ui.go_record(d, i, None);

                }
            } }
        if !w.is_empty() { span { class: "dim", if count == 0 { "none in the recording" } else { "{count} calls" } } }
    }
}
