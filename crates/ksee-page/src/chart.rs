//! The timeline. On top, an overview of the whole recording: which task
//! ran when, where each syscall starts, the window shown below, the cursor.
//! Below, the window as a stack chart (View::timeline): each step its
//! task's stack, the outermost at the bottom; a bar a call (or code inlined
//! into one) over the steps it is on, colored by the kernel's part it is in,
//! the calls at the cursor darker; the task strip under it; then the
//! transport. The wheel zooms vertically and pans horizontally; a drag pans.
//! Hovered, a bar is outlined and says what its call was (nothing else
//! moves); clicked, the cursor goes there. Find a function by any part
//! of its name.

use dioxus::prelude::*;
use ksee_core::arch::AREAS;
use ksee_core::trace::Record;
use ksee_core::view::{wheel_window, Timeline as Chart};
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
/// members neighbors: the core kernel warm (sched coral, kernel orange, mm
/// gold), storage green (fs, block), the machine blue (drivers, the port's
/// arch fainter), the helpers cool gray (lib, include fainter), so the
/// work stands out. A file in none of them: gray.
pub(crate) fn call_color(file: Option<&str>, on: bool) -> String {
    const COLORS: [(i32, i32); 9] = [
        (135, 40), (160, 40), // fs, block
        (50, 60),             // mm
        (10, 60), (32, 65),   // sched, kernel
        (215, 45), (215, 18), // drivers, arch
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
    // (The cursor out of the window: the window to it, as wide; slid just
    // enough to have it a tenth inside, from nearby (a step, a line); its
    // middle there, from afar (a link, another call).)
    let d = use_context::<Derived>();
    use_effect(move || {
        let Some(at) = d.cursor_step.cloned() else { return };
        let (a, span) = *d.window.peek();
        let margin = span / 10.0;
        if at < a {
            ui.view(if a - at < span { at - margin } else { at - span / 2.0 }, span);
        } else if at >= a + span {
            ui.view(if at - (a + span) < span { at + margin + 1.0 - span } else { at - span / 2.0 }, span);
        }
    });
    let title = d.run.cloned().map(|s| s.title()).unwrap_or_default();
    rsx! {
        section { class: "timeline",
            Overview {}
            div { class: "charthead",
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
    let cursor = d.cursor_step.cloned();
    let (n, tasks, starts) = m.view(|v| {
        let n = v.rec.steps.len().max(1);
        let tasks: Vec<i32> = (0..BUCKETS).map(|k| {
            let i = v.rec.record_of(k * n / BUCKETS);
            v.rec.tasks.get(i).map_or(0, |&t| v.task(t, i).pid())
        }).collect();
        let starts: Vec<usize> = v.rec.runs.iter().filter(|s| s.nr.is_some()).map(|s| v.rec.step_of(s.first)).collect();
        (n as f64, tasks, starts)
    });
    let (wa, span) = (d.window)();
    let pct = |s: f64| s * 100.0 / n;
    rsx! {
        div { class: "overview", title: "the whole recording: click to move the window there",
            onresize: move |e| if let Ok(s) = e.get_content_box_size() { width.set(s.width.max(1.0)) },
            onclick: move |e| ui.view(e.element_coordinates().x / width() * n - span / 2.0, span),
            for (k , pid) in tasks.into_iter().enumerate() {
                div { class: "b", style: "left: {k as f64 * 100.0 / BUCKETS as f64}%; width: {100.0 / BUCKETS as f64 + 0.1}%; background: {task_color(pid)}" }
            }
            for s in starts { div { class: "start", style: "left: {pct(s as f64)}%" } }
            div { class: "win", style: "left: {pct(wa)}%; width: {pct(span).max(0.3)}%" }
            if let Some(s) = cursor { div { class: "at", style: "left: {pct(s)}%" } }
        }
    }
}

/// A pan under way: where it began, and the time window and the rows'
/// scroll then.
#[derive(Clone, Copy)]
struct Pan {
    x: f64,
    y: f64,
    scroll: i32,
    window: (f64, f64),
    pointer_id: i32,
}

/// `text` in `color`, in the box at (`x`, `y`) `w` by `h`: clipped to it,
/// in the middle of its height.
fn label(ctx: &web_sys::CanvasRenderingContext2d, text: &str, color: &str, x: f64, y: f64, w: f64, h: f64) {
    ctx.save();
    ctx.begin_path();
    ctx.rect(x, y, w, h);
    ctx.clip();
    ctx.set_fill_style_str(color);
    let _ = ctx.fill_text(text, x + 3.0, y + h / 2.0);
    ctx.restore();
}

/// The page's element with id `id`.
pub(crate) fn element(id: &str) -> Option<web_sys::HtmlElement> {
    crate::document()?.get_element_by_id(id)?.dyn_into().ok()
}

/// A point on the page (client coordinates), on the chart's canvas.
fn chart_coordinates(client_x: f64, client_y: f64) -> Option<(f64, f64)> {
    let rect = element("timeline-canvas")?.get_bounding_client_rect();
    Some((client_x - rect.left(), client_y - rect.top()))
}

/// The chart's text color (the theme's): the outline's and the cross's.
fn foreground() -> String {
    element("timeline-chart").and_then(|chart| web_sys::window()?.get_computed_style(&chart).ok()??.get_property_value("color").ok())
        .filter(|color| !color.is_empty()).unwrap_or_else(|| "currentColor".into())
}

/// The stack rows' height: `depth` rows, at least the `room` there is.
fn chart_height(depth: usize, room: f64) -> f64 {
    (depth as f64 * ROW).max(room)
}

/// The task strip's spans' names and colors (the timeline's tasks', in order).
fn task_names(chart: &Chart) -> Vec<(String, String)> {
    let m = machine();
    m.view(|v| chart.tasks.iter().map(|&(_, end, task)| {
        let t = v.task(task, v.rec.steps[end - 1].0);
        (t.named(), task_color(t.pid()))
    }).collect())
}

/// Canvas `id`, `width` by `height` px (sharp at the screen's pixel
/// ratio), cleared: it and its 2D context.
fn canvas_context(id: &str, width: f64, height: f64) -> Option<(web_sys::HtmlCanvasElement, web_sys::CanvasRenderingContext2d)> {
    let canvas: web_sys::HtmlCanvasElement = element(id)?.dyn_into().ok()?;
    let ctx: web_sys::CanvasRenderingContext2d = canvas.get_context("2d").ok()??.dyn_into().ok()?;
    let dpr = web_sys::window().map_or(1.0, |w| w.device_pixel_ratio()).max(1.0);
    canvas.set_width((width * dpr).round() as u32);
    canvas.set_height((height * dpr).round() as u32);
    let _ = canvas.style().set_property("width", &format!("{width}px"));
    let _ = canvas.style().set_property("height", &format!("{height}px"));
    let _ = ctx.set_transform(dpr, 0.0, 0.0, dpr, 0.0, 0.0);
    ctx.clear_rect(0.0, 0.0, width, height);
    Some((canvas, ctx))
}

/// The window's steps, `width` px wide: step `s` at `(s - a) * width / span`.
#[derive(Clone, Copy)]
struct Scale {
    a: f64,
    span: f64,
    width: f64,
}

impl Scale {
    fn x(&self, s: f64) -> f64 {
        (s - self.a) * self.width / self.span
    }

    fn step(&self, x: f64) -> f64 {
        self.a + x / self.width * self.span
    }
}

/// The stack chart and the task strip drawn, the window `scale`: the bars
/// (`calls`, the stack's, darker; `find`'s, lit; `lit`, outlined), a bar
/// thinner than a pixel once per pixel; the cross at step `now` and
/// `level`.
fn draw_chart(
    chart: &Chart,
    tasks: &[(String, String)],
    scale: Scale,
    height: f64,
    calls: &[(usize, String)],
    find: &str,
    lit: Option<&(usize, String)>,
    now: Option<f64>,
    level: Option<usize>,
    locked: Option<usize>,
) {
    let width = scale.width;
    let (Some((canvas, ctx)), Some((_, task_ctx))) = (canvas_context("timeline-canvas", width, height), canvas_context("timeline-task-canvas", width, TASK_LANE))
        else { return };
    let (a, b) = (scale.a, scale.a + scale.span);
    // (A span's place on the canvas: its ends, the visible part's.)
    let place = |s0: usize, s1: usize| (scale.x(s0 as f64), scale.x(s1 as f64));
    let foreground = foreground();
    task_ctx.set_font("10px ui-monospace, SFMono-Regular, Menlo, monospace");
    task_ctx.set_text_baseline("middle");
    let k0 = chart.tasks.len() - chart.tasks.iter().rev().take_while(|s| s.1 as f64 > a).count();
    for (k, &(s0, s1, _)) in chart.tasks_in(a, b).iter().enumerate() {
        let (x0, x1) = place(s0, s1);
        let (name, color) = &tasks[k0 + k];
        task_ctx.set_fill_style_str(color);
        task_ctx.fill_rect(x0, 0.0, (x1 - x0 - 1.0).max(1.0), TASK_LANE);
        let (v0, v1) = (x0.max(0.0), x1.min(width));
        label(&task_ctx, name, "#1f2328", v0, 0.0, (v1 - v0 - 2.0).max(1.0), TASK_LANE);
    }
    ctx.set_font("11px ui-monospace, SFMono-Regular, Menlo, monospace");
    ctx.set_text_baseline("middle");
    // (For the tests: each call's bar, the middle of what is seen of it,
    // its left end and its width, once; the bars found; how far the bars
    // reach.)
    let mut points: Vec<String> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut found_count = 0;
    let mut reach = (width, 0.0f64);
    for d in 0..chart.depth(a, b) {
        let y = height - (d as f64 + 1.0) * ROW;
        // (The last pixel drawn on the row: a bar thinner than a pixel on it is not drawn again.)
        let mut drawn = f64::NEG_INFINITY;
        for &(s0, s1, bi) in chart.spans(d, a, b) {
            let (x0, x1) = place(s0, s1);
            if x1 - x0 < 1.0 && x0.floor() <= drawn {
                continue;
            }
            drawn = x1.floor().max(x0.floor());
            let bar = &chart.bars[bi as usize];
            let id = (bar.first, bar.name.clone());
            let on = calls.contains(&id);
            let found = !find.is_empty() && bar.name.to_lowercase().contains(find);
            let color = if found { "#ffd866".to_string() } else { call_color(bar.file.as_deref(), on) };
            let (v0, v1) = (x0.max(0.0), x1.min(width));
            found_count += found as usize;
            if crate::test::on() && seen.insert(id.clone()) {
                points.push(format!("{} {} {},{} {x0} {}", bar.name, bar.first, (v0 + v1) / 2.0, y + ROW / 2.0, x1 - x0));
            }
            reach = (reach.0.min(v0), reach.1.max(v1));
            ctx.set_fill_style_str(&color);
            ctx.fill_rect(x0, y, (x1 - x0 - 1.0).max(1.0), ROW - 1.0);
            if lit.is_some_and(|l| l.0 == id.0 && l.1 == id.1) {
                ctx.set_stroke_style_str(&foreground);
                ctx.set_line_width(1.5);
                ctx.stroke_rect(v0 + 0.75, y + 0.75, (v1 - v0 - 2.5).max(1.0), ROW - 2.5);
            }
            if v1 - v0 >= 24.0 {
                label(&ctx, &bar.name, if on { "#fff" } else { "#1f2328" }, v0, y, v1 - v0 - 2.0, ROW);
            }
        }
    }
    // (The moment shown, a cross: down, its step; across, under its frame's
    // level, the Stack's (under: its bars' names stay readable).)
    ctx.set_fill_style_str(&foreground);
    let now = now.filter(|&s| s >= a && s < b).map(|s| scale.x(s));
    if let Some(x) = now {
        ctx.fill_rect(x - 1.0, 0.0, 2.0, height);
        if let Some(level) = level {
            ctx.fill_rect(0.0, height - level as f64 * ROW - 1.0, width, 2.0);
        }
    }
    if crate::test::on() {
        // (data-test-NAME: what the chart drew, for the tests; none, gone.)
        let attr = |name: &str, value: Option<String>| {
            let name = format!("data-test-{name}");
            let _ = match value { Some(v) => canvas.set_attribute(&name, &v), None => canvas.remove_attribute(&name) };
        };
        attr("level", locked.map(|l| l.to_string()));
        attr("bars", Some(points.join("\n")));
        attr("found", Some(found_count.to_string()));
        attr("now", now.map(|n| n.to_string()));
        attr("reach", Some(format!("{},{}", reach.0, reach.1)));
        let names: Vec<&str> = chart.tasks_in(a, b).iter().enumerate().map(|(k, _)| tasks[k0 + k].0.as_str()).collect();
        attr("tasks", Some(names.join("\n")));
    }
}

#[component]
fn Bars(tip: Signal<Option<Tip>>) -> Element {
    let ui = use_context::<Ui>();
    let d = use_context::<Derived>();
    let chart = d.timeline.cloned();
    // (The task strip's names and colors: a snapshot's, once.)
    let tasks = use_memo(move || crate::state::Shared(std::rc::Rc::new(task_names(&d.timeline.read()))));
    let mut pan = use_signal(|| None::<Pan>);
    // (Latched through pointer-up: a click fires after it, when pan is
    // already none — so a drag never ends in a click.)
    let mut dragged = use_signal(|| false);
    let mut width = use_signal(|| 1000.0f64);
    let sizes = use_context::<Signal<crate::layout::Sizes>>();
    let (a, span) = (d.window)();
    let height = chart_height(chart.depth(a, a + span), sizes().chart);
    let scale = move || {
        let (a, span) = (d.window)();
        Scale { a, span, width: width() }
    };
    // (How far the stack rows are scrolled up from their bottom: kept as the
    // window moves and the chart grows or shrinks, so the bars stay where
    // they are, the outermost at the bottom; at first, none.)
    let mut from_bottom = use_signal(|| 0);
    // (The canvases mounted: then drawn.)
    let mut mounted = use_signal(|| false);
    use_effect(move || {
        mounted();
        let chart = d.timeline.cloned();
        let scale = scale();
        let height = chart_height(chart.depth(scale.a, scale.a + scale.span), sizes().chart);
        // (The calls on the moment's stack; the bar outlined: the one under
        // the pointer, else the frame shown's call.)
        let stack = d.stack.read();
        let calls: Vec<(usize, String)> = stack.iter().map(|f| (f.first, f.func.clone())).collect();
        let lit = tip().map(|t| t.call).or_else(|| stack.get(d.frame.cloned()).map(|f| (f.first, f.func.clone())));
        draw_chart(&chart, &tasks.read(), scale, height, &calls, &(ui.find)().to_lowercase(), lit.as_ref(),
                   d.cursor_step.cloned(), stack.len().checked_sub(1 + d.frame.cloned()),
                   ui.cursor.read().as_ref().and_then(|m| m.level));
        if let Some(scroll) = element("timeline-scroll") {
            scroll.set_scroll_top((scroll.scroll_height() - scroll.client_height() - *from_bottom.peek()).max(0));
        }
    });
    // (One rule for hover and click: the step under the pointer, and the
    // bar there, if any, its row.)
    let hit = move |x: f64, y: f64| -> Option<(usize, Option<(usize, usize)>)> {
        let s = scale().step(x).floor();
        let steps = machine().view(|v| v.rec.steps.len());
        let s = (s >= 0.0 && (s as usize) < steps).then_some(s as usize)?;
        let stack = d.timeline.peek().at(s);
        let dd = ((height - y) / ROW).floor();
        let bar = (y < height && dd >= 0.0 && (dd as usize) < stack.len()).then(|| (dd as usize, stack[dd as usize] as usize));
        Some((s, bar))
    };
    // (The wheel: up and down zoom around the pointer's step, in
    // proportion to how far it turned; left and right (or Shift) pan.)
    let wheel = move |x: f64, dx: f64, dy: f64, shift: bool| {
        let (start, span) = wheel_window((d.window)(), x, width(), dx, dy, shift, (dy * ZOOM).exp());
        ui.view(start, span);
    };
    // (For a screen reader: the bars and tasks the window has, once each.)
    let (b, labels, task_labels) = (a + span, std::collections::BTreeSet::new(), tasks.read().clone());
    let labels: Vec<String> = (0..chart.depth(a, b)).flat_map(|d| chart.spans(d, a, b).iter().map(|s| chart.bars[s.2 as usize].name.clone()))
        .fold(labels, |mut set, n| { set.insert(n); set }).into_iter().collect();
    let k0 = chart.tasks.len() - chart.tasks.iter().rev().take_while(|s| s.1 as f64 > a).count();
    let task_labels: Vec<String> = (0..chart.tasks_in(a, b).len()).map(|k| task_labels[k0 + k].0.clone()).collect();
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
                dragged.set(false);
                let p = e.client_coordinates();
                let scroll = element("timeline-scroll").map_or(0, |e| e.scroll_top());
                let pointer_id = e.pointer_id();
                if let Some(chart) = element("timeline-chart") {
                    let _ = chart.set_pointer_capture(pointer_id);
                }
                pan.set(Some(Pan { x: p.x, y: p.y, scroll, window: (d.window)(), pointer_id }));
            },
            onpointermove: move |e: PointerEvent| {
                if let Some(p) = pan() {
                    if p.pointer_id != e.pointer_id() { return }
                    let at = e.client_coordinates();
                    let (dx, dy) = (at.x - p.x, at.y - p.y);
                    // (Past 4 px: a drag, the window and the rows moving with the pointer.)
                    if dx.hypot(dy) >= 4.0 || *dragged.peek() {
                        if !*dragged.peek() {
                            dragged.set(true);
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
                        let (start, span) = p.window;
                        ui.view(start - dx / width() * span, span);
                        return;
                    }
                }
                // (Else, over a bar, what its call was: outlined, a tip.)
                let p = e.client_coordinates();
                let Some((x, y)) = chart_coordinates(p.x, p.y) else { return };
                let Some((_, bar)) = hit(x, y) else { return };
                let chart = d.timeline.peek();
                match bar.map(|(_, b)| &chart.bars[b]) {
                    None => if tip.peek().is_some() { tip.set(None) },
                    Some(b) => match tip() {
                        Some(t) if t.call.0 == b.first && t.call.1 == b.name => tip.set(Some(Tip { x: p.x, y: p.y, ..t })),
                        _ => tip.set(Some(Tip { x: p.x, y: p.y, call: (b.first, b.name.clone()), lines: said(b.first, &b.name, b.inlined) })),
                    },
                }
            },
            onpointerup: move |e| {
                if let Some(p) = pan().filter(|p| p.pointer_id == e.pointer_id()) {
                    pan.set(None);
                    if let Some(chart) = element("timeline-chart") {
                        let _ = chart.release_pointer_capture(p.pointer_id);
                    }
                }
            },
            // (A click, not a drag: the cursor to the step under it, at the
            // level of the bar clicked; off the bars, none: the innermost.)
            onclick: move |e: MouseEvent| {
                if dragged() { dragged.set(false); return }
                let p = e.client_coordinates();
                let Some((step, bar)) = chart_coordinates(p.x, p.y).and_then(|(x, y)| hit(x, y)) else { return };
                let i = machine().view(|v| v.rec.steps[step].0);
                if let Some(moment) = Ui::moment_at(i, Some(step), bar.map(|b| b.0)) {
                    ui.commit(Some(moment));
                }
            },
            onpointercancel: move |e: PointerEvent| {
                if pan().is_some_and(|p| p.pointer_id == e.pointer_id()) {
                    pan.set(None);
                    dragged.set(false);
                }
            },
            onmouseleave: move |_| {
                if pan().is_none() {
                        tip.set(None);
                }
            },
            div { id: "timeline-scroll", class: "chartscroll",
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
        let frame = v.call_stack(first, None, 0, first).into_iter().find(|f| !f.inlined);
        let args: Vec<(String, String)> = frame.map_or_else(Vec::new, |f| f.args.iter().map(|a| (a.name.clone(), a.shown())).collect());
        let ret = v.rec.calls.call_end(first).and_then(|end| match v.rec.records.get(end) {
            Some(&Record::Return { value, .. }) => value,
            _ => None,
        }).map(|r| format!(" = {}", v.tables.types.result(r))).unwrap_or_default();
        // (A syscall's entry: the syscall, its arguments and result; not a
        // run between syscalls that starts there.)
        let syscall = v.rec.runs.binary_search_by_key(&first, |s| s.first).ok().and_then(|k| v.run(k)).filter(|s| s.nr.is_some());
        match syscall {
            Some(s) => {
                let ret = s.ret.map_or_else(|| "…".into(), |r| v.tables.types.result(r as i64));
                let shown: Vec<String> = args.iter().map(|(_, x)| x.clone()).collect();
                vec![(format!("{}({}) = {ret}", s.name, shown.join(", ")), ""), (format!("a syscall by {}", s.task.named()), "dim")]
            }
            None => std::iter::once((format!("{name}{ret}"), "")).chain(args.into_iter().map(|(n, x)| (format!("{n} = {x}"), "f"))).collect(),
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
    let calls: Vec<usize> = if w.is_empty() { Vec::new() } else {
        m.view(|v| v.rec.find_calls(v.tables, |n| n.to_lowercase().contains(&w), 0, usize::MAX, None).collect())
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
                    ui.go_record(i);
                }
            } }
        if !w.is_empty() { span { class: "dim", if count == 0 { "none in the recording" } else { "{count} calls" } } }
    }
}
