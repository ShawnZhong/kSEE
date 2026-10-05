//! The timeline. On top, an overview of the whole recording: which task
//! ran when, where each syscall starts, the window shown below, the cursor.
//! Below, the window as a stack chart: columns of moments, each its task's
//! stack, the outermost at the bottom; a bar a call (or code inlined into
//! one) over the columns it is on, colored by the kernel's part it is in,
//! the calls at the cursor darker; the task strip under it; then the
//! transport. The wheel zooms vertically and pans horizontally; a drag pans.
//! Hovered, a moment is shown everywhere but the source's cursor line, and a
//! bar says what its call was; clicked, the cursor goes there. Find a
//! function by any part of its name.

use dioxus::prelude::*;
use ksee_core::arch::AREAS;
use ksee_core::trace::Record;
use ksee_core::view::{clamped_pan, wheel_window, Chart};
use wasm_bindgen::JsCast;

use crate::machine;
use crate::state::{Derived, Ui};

/// The overview's buckets.
const BUCKETS: usize = 240;
/// A stack row's fixed height (px); deeper charts scroll.
const ROW: f64 = 15.0;
/// How much a vertical wheel turn (a pixel of it) zooms.
const ZOOM: f64 = 0.002;
/// The task lane fixed under the stack rows (px).
const TASK_LANE: f64 = 12.0;

/// A task's hue, by its pid (the strip, the overview, the program's lanes):
/// one of a few calm ones, neighbors apart.
pub fn hue(pid: i32) -> i32 {
    const HUES: [i32; 8] = [210, 30, 160, 260, 95, 15, 190, 45];
    HUES[pid.unsigned_abs() as usize % HUES.len()]
}

/// A task's color: the idle task's (0, the CPU waiting) gray.
fn task_color(pid: i32) -> String {
    if pid == 0 { "hsl(0 0% 82%)".into() } else { format!("hsl({} 50% 62%)", hue(pid)) }
}

/// A call's color, by the first kernel area its source file belongs to
/// (AREAS: a hue and its saturation, each). A concept, a family, its
/// members neighbors: files and storage blue (fs, block), memory green,
/// tasks warm (sched coral, kernel amber), the machine teal (drivers, the
/// port's arch fainter), the helpers cool gray (lib, include fainter), so
/// the work stands out. A file in none of them: gray.
pub(crate) fn call_color(file: Option<&str>, on: bool) -> String {
    const COLORS: [(i32, i32); 9] = [
        (210, 55), (228, 45), // fs, block
        (135, 45),            // mm
        (18, 60), (38, 60),   // sched, kernel
        (180, 40), (180, 16), // drivers, arch
        (210, 12), (210, 5),  // lib, include
    ];
    let l = if on { 50 } else { 74 };
    match AREAS.iter().position(|(p, _)| file.unwrap_or("").starts_with(p)).and_then(|k| COLORS.get(k)) {
        Some(&(h, s)) => format!("hsl({h} {}% {l}%)", if on { s + 10 } else { s }),
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
    let mut ui = use_context::<Ui>();
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
    let title = d
        .syscall
        .cloned()
        .map(|s| format!("{} by {}", s.name, s.task.named()))
        .unwrap_or_default();
    rsx! {
        section { class: "history",
            Overview {}
            div { class: "flamehead",
                span { class: "dim", "{title}"
                    if (ui.win)().is_some() { " · " a { title: "the whole recording", onclick: move |_| ui.win.set(None), "all" } }
                }
                Find {}
                span { class: "legend",
                    for (p , n) in AREAS { span { i { style: "background: {call_color(Some(p), false)}" } "{n}" } }
                }
            }
            Bars { tip }
            crate::stack::Transport {}
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
        let n = v.rec.steps.len().max(1);
        let tasks: Vec<i32> = (0..BUCKETS)
            .map(|k| {
                let i = v.rec.record_of(k * n / BUCKETS);
                v.rec
                    .tasks
                    .get(i)
                    .and_then(|&t| v.task(t, i).pid)
                    .unwrap_or(0)
            })
            .collect();
        let starts: Vec<usize> = v
            .rec
            .syscalls
            .iter()
            .map(|s| v.rec.step_of(s.first))
            .collect();
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
            div { class: "win", style: "left: {pct(wa)}%; width: {pct(wb + 1 - wa).max(0.3)}%" }
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
    pointer_id: i32,
}

pub(crate) fn element(id: &str) -> Option<web_sys::HtmlElement> {
    web_sys::window()?
        .document()?
        .get_element_by_id(id)?
        .dyn_into()
        .ok()
}

fn chart_coordinates(client_x: f64, client_y: f64) -> Option<(f64, f64)> {
    let rect = element("timeline-canvas")?.get_bounding_client_rect();
    Some((client_x - rect.left(), client_y - rect.top()))
}

fn foreground() -> String {
    let Some(window) = web_sys::window() else {
        return "currentColor".into();
    };
    let Some(chart) = element("timeline-chart") else {
        return "currentColor".into();
    };
    window
        .get_computed_style(&chart)
        .ok()
        .flatten()
        .and_then(|style| style.get_property_value("color").ok())
        .filter(|color| !color.is_empty())
        .unwrap_or_else(|| "currentColor".into())
}

fn chart_height(depth: usize, room: f64) -> f64 {
    (depth as f64 * ROW).max(room)
}

#[derive(Clone)]
struct TaskRun {
    start: usize,
    end: usize,
    name: String,
    color: String,
}

fn task_runs(chart: &Chart) -> Vec<TaskRun> {
    let m = machine();
    chart
        .task_runs()
        .into_iter()
        .map(|(start, end, task)| {
            let t = m.view(|v| v.task(task, chart.cols[end - 1].i));
            TaskRun {
                start,
                end,
                name: t.named(),
                color: task_color(t.pid.unwrap_or(0)),
            }
        })
        .collect()
}

fn canvas_context(
    id: &str,
    width: f64,
    height: f64,
) -> Option<(
    web_sys::HtmlCanvasElement,
    web_sys::CanvasRenderingContext2d,
)> {
    let canvas: web_sys::HtmlCanvasElement = element(id)?.dyn_into().ok()?;
    let ctx = canvas
        .get_context("2d")
        .ok()
        .flatten()?
        .dyn_into::<web_sys::CanvasRenderingContext2d>()
        .ok()?;
    let dpr = web_sys::window()
        .map_or(1.0, |w| w.device_pixel_ratio())
        .max(1.0);
    canvas.set_width((width * dpr).round() as u32);
    canvas.set_height((height * dpr).round() as u32);
    let _ = canvas.style().set_property("width", &format!("{width}px"));
    let _ = canvas
        .style()
        .set_property("height", &format!("{height}px"));
    let _ = ctx.set_transform(dpr, 0.0, 0.0, dpr, 0.0, 0.0);
    ctx.clear_rect(0.0, 0.0, width, height);
    Some((canvas, ctx))
}

fn draw_chart(
    chart: &Chart,
    tasks: &[TaskRun],
    width: f64,
    height: f64,
    calls: &[(usize, String)],
    find: &str,
    lit: Option<&(usize, String)>,
    now: Option<f64>,
    level: Option<usize>,
    locked: Option<usize>,
    offset: f64,
) {
    let Some((canvas, ctx)) = canvas_context("timeline-canvas", width, height) else {
        return;
    };
    let Some((_, task_ctx)) = canvas_context("timeline-task-canvas", width, TASK_LANE)
    else {
        return;
    };
    let n = chart.cols.len().max(1) as f64;
    let foreground = foreground();
    task_ctx.set_font("10px ui-monospace, SFMono-Regular, Menlo, monospace");
    task_ctx.set_text_baseline("middle");
    for task in tasks {
        let x = task.start as f64 * width / n + offset;
        let w = (task.end - task.start) as f64 * width / n;
        if x + w <= 0.0 || x >= width {
            continue;
        }
        task_ctx.set_fill_style_str(&task.color);
        task_ctx.fill_rect(x, 0.0, (w - 1.0).max(1.0), TASK_LANE);
        task_ctx.save();
        task_ctx.begin_path();
        task_ctx.rect(x, 0.0, (w - 2.0).max(1.0), TASK_LANE);
        task_ctx.clip();
        task_ctx.set_fill_style_str("#1f2328");
        let _ = task_ctx.fill_text(&task.name, x + 3.0, TASK_LANE / 2.0);
        task_ctx.restore();
    }
    ctx.set_font("11px ui-monospace, SFMono-Regular, Menlo, monospace");
    ctx.set_text_baseline("middle");
    // (For the tests: each function's first bar, its middle; the bars found.)
    let mut points: Vec<String> = Vec::new();
    let mut found_count = 0;
    for (dd, a, e, b) in chart.bar_runs() {
        let bar = &chart.bars[b as usize];
        let id = (bar.first, bar.name.clone());
        let on = calls.contains(&id);
        let found = !find.is_empty() && bar.name.to_lowercase().contains(find);
        let color = if found {
            "#ffd866".to_string()
        } else {
            call_color(bar.file.as_deref(), on)
        };
        let x = a as f64 * width / n + offset;
        let w = (e - a) as f64 * width / n;
        let y = height - (dd as f64 + 1.0) * ROW;
        if found {
            found_count += 1;
        }
        if crate::test::on() && !points.iter().any(|p| p.split(' ').next() == Some(&bar.name)) {
            points.push(format!("{} {},{}", bar.name, x + w / 2.0, y + ROW / 2.0));
        }
        if x + w <= 0.0 || x >= width {
            continue;
        }
        ctx.set_fill_style_str(&color);
        ctx.fill_rect(x, y, (w - 1.0).max(1.0), ROW - 1.0);
        if lit.is_some_and(|l| l.0 == id.0 && l.1 == id.1) {
            ctx.set_stroke_style_str(&foreground);
            ctx.set_line_width(1.5);
            ctx.stroke_rect(x + 0.75, y + 0.75, (w - 2.5).max(1.0), ROW - 2.5);
        }
        if w >= 24.0 {
            ctx.save();
            ctx.begin_path();
            ctx.rect(x, y, w - 2.0, ROW);
            ctx.clip();
            ctx.set_fill_style_str(if on { "#fff" } else { "#1f2328" });
            let _ = ctx.fill_text(&bar.name, x + 3.0, y + ROW / 2.0);
            ctx.restore();
        }
    }
    // (The moment shown, a cross: down, its step; across, under its frame's
    // level, the Stack's (under: its bars' names stay readable).)
    ctx.set_fill_style_str(&foreground);
    if let Some(pct) = now {
        ctx.fill_rect(pct * width / 100.0 + offset - 1.0, 0.0, 2.0, height);
        if let Some(level) = level {
            ctx.fill_rect(0.0, height - level as f64 * ROW - 1.0, width, 2.0);
        }
    }
    if crate::test::on() {
        match locked {
            Some(level) => canvas.set_attribute("data-test-level", &level.to_string()),
            None => canvas.remove_attribute("data-test-level"),
        }.ok();
        let _ = canvas.set_attribute("data-test-bars", &points.join("\n"));
        let _ = canvas.set_attribute("data-test-found", &found_count.to_string());
        match now {
            Some(now) => canvas.set_attribute("data-test-now", &now.to_string()),
            None => canvas.remove_attribute("data-test-now"),
        }.ok();
        let task_names: Vec<&str> = tasks.iter().map(|t| t.name.as_str()).collect();
        let _ = canvas.set_attribute("data-test-tasks", &task_names.join("\n"));
    }
}

#[component]
fn Bars(tip: Signal<Option<Tip>>) -> Element {
    let ui = use_context::<Ui>();
    let d = use_context::<Derived>();
    let chart = d.chart.cloned();
    let steps = machine().view(|v| v.rec.steps.len().max(1));
    // (The task strip's runs: the labels' and the drawing's.)
    let runs = use_memo(move || crate::state::Shared(std::rc::Rc::new(task_runs(&d.chart.read()))));
    let tasks = runs.cloned().0;
    let mut pan = use_signal(|| None::<Pan>);
    // (Latched through pointer-up: a click fires after it, when pan is
    // already none — so a drag never ends in a click.)
    let mut dragged = use_signal(|| false);
    let mut width = use_signal(|| 1000.0f64);
    let depth = chart.cols.iter().map(|c| c.frames.len()).max().unwrap_or(0);
    let sizes = use_context::<Signal<crate::layout::Sizes>>();
    let height = chart_height(depth, sizes().chart);
    // (How far the stack rows are scrolled up from their bottom: kept as the
    // window moves and the chart grows or shrinks, so the bars stay where
    // they are, the outermost at the bottom; at first, none.)
    let mut from_bottom = use_signal(|| 0);
    // (A drag's pan so far (px), drawn until it ends; and the canvases
    // mounted: either draws again.)
    let mut offset = use_signal(|| 0.0f64);
    let mut mounted = use_signal(|| false);
    use_effect(move || {
        mounted();
        let chart = d.chart.cloned();
        let height = chart_height(chart.cols.iter().map(|c| c.frames.len()).max().unwrap_or(0), sizes().chart);
        let (sa, sb) = (d.window)();
        // (Where a step is, across the window (%): the moment shown's (the
        // pointer's, else the cursor's); the calls on its stack.)
        let x = |s: f64| (s >= sa as f64 && s < (sb + 1) as f64).then(|| (s - sa as f64) * 100.0 / (sb - sa + 1) as f64);
        let stack = d.stack.read();
        let calls: Vec<(usize, String)> = stack.iter().map(|f| (f.first, f.func.clone())).collect();
        // (The bar outlined: the one under the pointer, else the frame shown's call.)
        let lit = tip().map(|t| t.call).or_else(|| stack.get(d.frame.cloned()).map(|f| (f.first, f.func.clone())));
        draw_chart(&chart, &runs.read().0, width(), height, &calls, &(ui.find)().to_lowercase(), lit.as_ref(),
                   d.preview.cloned().or(d.cursor.cloned()).and_then(x), stack.len().checked_sub(1 + d.frame.cloned()),
                   ui.at.read().as_ref().and_then(|m| m.frame), offset());
        if let Some(scroll) = element("timeline-scroll") {
            scroll.set_scroll_top((scroll.scroll_height() - scroll.client_height() - *from_bottom.peek()).max(0));
        }
    });
    // (One rule for hover and click: the bar under the pointer, and the
    // moment that column samples; off the bars, at level `off`.)
    let hit = move |chart: &Chart,
                    x: f64,
                    y: f64,
                    off: Option<usize>|
          -> Option<(Option<crate::state::Moment>, Option<usize>)> {
        let k = ((x / width() * chart.cols.len() as f64).max(0.0) as usize)
            .min(chart.cols.len().checked_sub(1)?);
        let dd = ((height - y) / ROW).floor();
        let bar = (y < height && dd >= 0.0 && (dd as usize) < chart.cols[k].frames.len())
            .then(|| chart.cols[k].frames[dd as usize] as usize);
        let frame = bar.map(|_| dd as usize).or(off);
        Some((
            Ui::moment_at(d, chart.cols[k].i, Some(chart.cols[k].step), frame),
            bar,
        ))
    };
    // (The wheel: up and down zoom around the pointer's step, in
    // proportion to how far it turned; left and right (or Shift) pan.)
    let wheel = move |x: f64, dx: f64, dy: f64, shift: bool| {
        let (start, span) = wheel_window((d.window)(), x, width(), dx, dy, shift, (dy * ZOOM).exp());
        ui.view(start, span);
    };
    let labels: Vec<String> = chart.bars.iter().map(|bar| bar.name.clone()).collect();
    let task_labels: Vec<String> = tasks.iter().map(|task| task.name.clone()).collect();
    rsx! {
        div { id: "timeline-chart", class: "chart",
            onresize: move |e| if let Ok(s) = e.get_content_box_size() { width.set(s.width.max(1.0)) },
            onwheel: move |e| {
                e.prevent_default();
                let d = e.delta().strip_units();
                let p = e.client_coordinates();
                if let Some((x, _)) = chart_coordinates(p.x, p.y) {
                    wheel(x, d.x, d.y, e.modifiers().shift());
                }
            },
            onpointerdown: move |e| {
                e.prevent_default();
                tip.set(None);
                ui.set_preview(None);
                dragged.set(false);
                let p = e.client_coordinates();
                let scroll = element("timeline-scroll").map_or(0, |e| e.scroll_top());
                let (a, b) = (d.window)();
                let pointer_id = e.pointer_id();
                if let Some(chart) = element("timeline-chart") {
                    let _ = chart.set_pointer_capture(pointer_id);
                }
                pan.set(Some(Pan { x: p.x, y: p.y, scroll, a, b, pointer_id }));
            },
            onpointermove: {
                let chart = chart.clone();
                move |e: PointerEvent| {
                    if let Some(p) = pan() {
                        if p.pointer_id != e.pointer_id() { return }
                        let at = e.client_coordinates();
                        let (dx, dy) = (at.x - p.x, at.y - p.y);
                        // (Past 4 px: a drag.)
                        if dx.hypot(dy) >= 4.0 || *dragged.peek() {
                            if !*dragged.peek() {
                                dragged.set(true);
                                ui.set_preview(None);
                                tip.set(None);
                            }
                            // (Up and down: the rows, their distance from the bottom
                            // kept before the redraw the pan makes restores it.)
                            if let Some(scroll) = element("timeline-scroll") {
                                let max = scroll.scroll_height() - scroll.client_height();
                                let top = ((p.scroll as f64 - dy).round() as i32).clamp(0, max);
                                from_bottom.set(max - top);
                                scroll.set_scroll_top(top);
                            }
                            offset.set(clamped_pan((p.a, p.b), dx, width(), steps).2);
                            return;
                        }
                    }
                    // (Else the moment under the pointer shown everywhere but the line: over a bar, what its call was.)
                    let p = e.client_coordinates();
                    let Some((x, y)) = chart_coordinates(p.x, p.y) else { return };
                    // (Off the bars, the level chosen, kept.)
                    let Some((peek, bar)) = hit(&chart, x, y, ui.at.peek().as_ref().and_then(|m| m.frame)) else { return };
                    ui.set_preview(peek);
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
                if let Some(p) = pan().filter(|p| p.pointer_id == e.pointer_id()) {
                    let dx = e.client_coordinates().x - p.x;
                    pan.set(None);
                    offset.set(0.0);
                    if *dragged.peek() && dx != 0.0 {
                        let (start, span, _) = clamped_pan((p.a, p.b), dx, width(), steps);
                        ui.view(start, span);
                    }
                    if let Some(chart) = element("timeline-chart") {
                        let _ = chart.release_pointer_capture(p.pointer_id);
                    }
                }
            },
            // (A click, not a drag: resolve its coordinates afresh, then
            // commit; off the bars, no level chosen: the innermost.)
            onclick: {
                let chart = chart.clone();
                move |e: MouseEvent| {
                    if dragged() { dragged.set(false); return }
                    let p = e.client_coordinates();
                    if let Some(moment) = chart_coordinates(p.x, p.y).and_then(|(x, y)| hit(&chart, x, y, None)).and_then(|h| h.0) {
                        ui.commit(Some(moment));
                    }
                }
            },
            onpointercancel: move |e: PointerEvent| {
                if pan().is_some_and(|p| p.pointer_id == e.pointer_id()) {
                    pan.set(None);
                    dragged.set(false);
                    offset.set(0.0);
                }
            },
            onmouseleave: move |_| {
                if pan().is_none() {
                    ui.set_preview(None);
                    tip.set(None);
                }
            },
            div { id: "timeline-scroll", class: "flamescroll",
                onscroll: move |_| if let Some(s) = element("timeline-scroll") { from_bottom.set(s.scroll_height() - s.client_height() - s.scroll_top()) },
                canvas { id: "timeline-canvas", class: "timeline-canvas", role: "img", aria_label: "Call stack timeline" }
            }
            canvas { id: "timeline-task-canvas", class: "timeline-canvas task-canvas", role: "img", aria_label: "Task runs in the call stack timeline",
                onmounted: move |_| mounted.set(true)
            }
            div { class: "sr-only",
                p { "Call labels in the timeline:" }
                ul { for label in labels { li { "{label}" } } }
                p { "Task runs in the timeline:" }
                ul { for task in task_labels { li { "{task}" } } }
            }
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
    m.view(|v| {
        let frame = v
            .call_stack(first, None, 0, first)
            .into_iter()
            .find(|f| !f.inlined);
        let args: Vec<(String, String)> = frame
            .as_ref()
            .map(|f| f.args.clone())
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.name.clone(), a.shown()))
            .collect();
        let ret = v
            .rec
            .calls
            .call_end(first)
            .and_then(|end| match v.rec.records.get(end) {
                Some(&Record::Return { value, .. }) => value,
                _ => None,
            });
        let ret = ret
            .map(|r| format!(" = {}", v.tables.types.result(r)))
            .unwrap_or_default();
        match v.rec.syscalls.binary_search_by_key(&first, |s| s.first).ok().and_then(|k| v.syscall(k)) {
            Some(s) => {
                let ret = s.ret.map_or_else(|| "…".into(), |r| v.tables.types.result(r as i64));
                let shown: Vec<String> = args.iter().map(|(_, x)| x.clone()).collect();
                vec![
                    (format!("{}({}) = {ret}", s.name, shown.join(", ")), ""),
                    (format!("a syscall by {}", s.task.named()), "dim"),
                ]
            }
            None => std::iter::once((format!("{name}{ret}"), ""))
                .chain(args.into_iter().map(|(n, x)| (format!("{n} = {x}"), "f")))
                .collect(),
        }
    })
}


/// Find a function by any part of its name: its bars lit, its calls in the
/// recording counted; Enter, the cursor at its next call (Shift: the one
/// before), round to the start.
#[component]
fn Find() -> Element {
    let mut ui = use_context::<Ui>();
    let d = use_context::<Derived>();
    (ui.snapshot)();
    let w = (ui.find)().to_lowercase();
    let m = machine();
    let calls: Vec<usize> = if w.is_empty() {
        Vec::new()
    } else {
        m.view(|v| {
            v.rec
                .calls(
                    v.tables,
                    |n| n.to_lowercase().contains(&w),
                    0,
                    usize::MAX,
                    None,
                )
                .collect()
        })
    };
    let count = calls.len();
    rsx! {
        input { class: "find mono", placeholder: "find a function…", value: "{ui.find}", spellcheck: false,
            title: "any part of a function's name (its bars colored as you type): Enter, its next call after the cursor; Shift+Enter, the one before; Esc clears",
            oninput: move |e| ui.find.set(e.value()),
            onkeydown: move |e| {
                if e.key() == Key::Escape { ui.find.set(String::new()); return }
                if e.key() != Key::Enter || calls.is_empty() { return }
                let here = d.line.cloned().map_or(0, |l| l.i);
                let to = if e.modifiers().shift() {
                    calls.iter().rev().find(|&&c| c < here).or(calls.last())
                } else {
                    calls.iter().find(|&&c| c > here).or(calls.first())
                };
                if let Some(&i) = to {
                    ui.go_record(d, i);

                }
            } }
        if !w.is_empty() { span { class: "dim", if count == 0 { "none in the recording" } else { "{count} calls" } } }
    }
}
