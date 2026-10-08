//! The moment shown: the transport (where it is, the steps), and the
//! Stack: the calls then, innermost first, each with its arguments and
//! locals as memory was then; a value opens into its members, or what it
//! points to, and says who last wrote it.

use std::collections::BTreeSet;

use dioxus::prelude::*;
use ksee_core::view::{step, Object};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;

use crate::machine;
use crate::state::{Derived, Moment, Tab, Ui};

/// A move of the cursor in time, as a debugger's steps. Into and Back: the
/// line after or before, wherever it is (into the calls: the innermost
/// frame then). Over and BackOver: the line after or before in the
/// function you read (the level chosen), over what it calls (on in its
/// caller once it returns). Out: on until the function read returns, its
/// caller's next line.
#[derive(Clone, Copy)]
pub enum Move {
    Back,
    Into,
    BackOver,
    Over,
    Out,
}

/// The cursor moved by `m`; past the run's lines, into its task's run after
/// it (or before): its next syscall, or what it did between.
pub fn walk(ui: Ui, d: Derived, m: Move) {
    let Some(at) = ui.cursor.peek().clone() else { return };
    let k = at.run;
    let lines = Ui::run_lines(k);
    let depth = d.stack.peek().get(*d.frame.peek()).map_or(0, |f| f.depth);
    let (dir, within) = match m {
        Move::Back => (-1, None),
        Move::Into => (1, None),
        Move::BackOver => (-1, Some(depth)),
        Move::Over => (1, Some(depth)),
        // (Out of the outermost: past the run.)
        Move::Out => (1, Some(depth.wrapping_sub(1))),
    };
    let to = if matches!(m, Move::Out) && depth == 0 { None } else { step(&lines, at.line, dir, within) };
    if let Some(j) = to {
        match m {
            Move::Back | Move::Into | Move::Out => ui.commit(Some(Moment { run: k, line: j, level: None })),
            Move::BackOver | Move::Over => ui.go(j),
        }
        return;
    }
    let next = machine().view(|v| {
        let all = &v.rec.runs;
        let task = all.get(k)?.task;
        let mine: Vec<usize> = (0..all.len()).filter(|&j| all[j].task == task).collect();
        let at = mine.iter().position(|&j| j == k)?;
        mine.get(at.checked_add_signed(dir)?).copied()
    });
    if let Some(next) = next {
        ui.select(next);
        if dir < 0 {
            ui.go(Ui::run_lines(next).len().saturating_sub(1));
        }
    }
}

/// Under the strip: the steps (Move), in a debugger's order and with its
/// icons: ↶ Back (←; Shift+←, back over), ↷ Over (Shift+→), ↓ Into (→),
/// ↑ Out (↑); and where the
/// cursor is: the frame you read (chosen on the Stack or the chart; else the
/// innermost).
#[component]
pub fn Transport() -> Element {
    let ui = use_context::<Ui>();
    let d = use_context::<Derived>();
    let here = d.line.cloned();
    let frames = d.stack.cloned();
    let chosen = d.frame.cloned();
    let place = frames.get(chosen).map_or(String::new(), |f| format!("{}{}", f.func, f.at.as_ref().map_or(String::new(), |(file, line)| format!(" {file}:{line}"))));
    // (On the innermost frame: a line that runs more than once in this
    // call, which turn the cursor is on; a step that lands after a call,
    // what it returned.)
    let here = here.filter(|_| chosen == 0);
    let turn = here.as_ref().filter(|l| l.turn > 1).map_or(String::new(), |l| format!(" · turn {}", l.turn));
    let returned = here.as_ref().and_then(|l| l.called.as_ref().filter(|c| c.at <= l.at))
        .map_or(String::new(), |c| format!(" · {} returned {}", c.func, machine().tables().types.result(c.value)));
    rsx! {
        div { class: "transport",
            button { title: "Step back: the line before, wherever it is (←; Shift+←: back over, in the function you read)", onclick: move |_| walk(ui, d, Move::Back), "↶ Back" }
            button { title: "Step over: the next line of the function you read, over what it calls (Shift+→)", onclick: move |_| walk(ui, d, Move::Over), "↷ Over" }
            button { title: "Step into: the very next line, into the calls (→)", onclick: move |_| walk(ui, d, Move::Into), "↓ Into" }
            button { title: "Step out: on until the function you read returns, its caller's next line (↑)", onclick: move |_| walk(ui, d, Move::Out), "↑ Out" }
            span { class: "where mono dim", "{place}{turn}{returned}" }
        }
    }
}

/// The page's keys, anywhere on it: one listener on the window; → Into, ←
/// Back (Shift: Over, back over), ↑ Out (see Move). Not in text fields (the lesson's
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
            // (← →: once the kernel is up.)
            let up = matches!(*ui.kernel.peek(), Some(Ok(())));
            let arrow = match e.key().as_str() {
                "ArrowLeft" if up => { walk(ui, d, if e.shift_key() { Move::BackOver } else { Move::Back }); true }
                "ArrowRight" if up => { walk(ui, d, if e.shift_key() { Move::Over } else { Move::Into }); true }
                "ArrowUp" if up => { walk(ui, d, Move::Out); true }
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

/// The calls at the moment shown, at the bottom of the pane as the chart
/// stacks them (the outermost lowest); under the frame chosen, its
/// arguments and locals as memory was then. A click on another: it chosen.
/// Under them, the task running (first: the pane is reversed, see its CSS).
#[component]
pub fn Stack() -> Element {
    let mut ui = use_context::<Ui>();
    (ui.snapshot)();
    // (The stack at the moment shown: the pointer's over the chart, else the cursor's.)
    let d = use_context::<Derived>();
    let frames = d.stack.cloned();
    let chosen = d.frame.cloned();
    let at = d.line.cloned().map_or(0, |l| l.at);
    let task = machine().view(|v| v.rec.tasks.get(at).map(|&t| v.task(t, at)));
    let mut open = use_context::<Signal<BTreeSet<String>>>();
    // (The chosen frame, its row and values, scrolled into view: as little
    // as it takes, its row first.)
    use_effect(move || {
        let _ = (d.frame.read(), d.stack.read());
        let doc = crate::document();
        let pick = |s: &str| doc.as_ref().and_then(|d| d.query_selector(s).ok().flatten());
        let (Some(pane), Some(on)) = (pick(".stack"), pick(".stack tbody.on")) else { return };
        let (p, r) = (pane.get_bounding_client_rect(), on.get_bounding_client_rect());
        // (Above the task's line, kept at the bottom.)
        let bottom = p.bottom() - pick(".stack .current").map_or(0.0, |c| c.get_bounding_client_rect().height());
        let by = if r.top() < p.top() { r.top() - p.top() } else { (r.bottom() - bottom).max(0.0).min(r.top() - p.top()) };
        pane.set_scroll_top(pane.scroll_top() + by as i32);
    });
    rsx! {
        div { class: "stack",
            if let Some(t) = task {
                div { class: "current dim", "current task: "
                    a { class: "mono", title: "its task_struct, in Tasks (the task running: open there)",
                        onclick: move |_| { ui.tab.set(Tab::Tasks); open.write().remove(&format!("task/{}", t.addr)); },
                        "{t.named()}" }
                }
            }
            table { class: "fields",
                for (k , f) in frames.iter().cloned().enumerate() {
                    tbody { class: if k == chosen { "on" } else { "" },
                    tr { class: if k == chosen { "frame on" } else { "frame" }, onclick: {
                        let level = frames.len() - 1 - k;
                        move |_| {
                            let at = ui.cursor.peek().clone();
                            if let Some(at) = at { ui.commit(Some(Moment { level: Some(level), ..at })) }
                        }
                    },
                        td { colspan: "3",
                            a { class: "mono", "{f.func}" }
                            if let Some((file, line)) = &f.at { span { class: "dim mono", " {file}:{line}" } }
                            if f.inlined { span { class: "dim", " · inlined" } }
                        }
                    }
                    if k == chosen {
                        for o in f.args.into_iter().chain(f.locals) {
                            Value { o: o.clone(), path: format!("{k}.{}", o.name), depth: 1, at }
                        }
                    }
                    }
                }
            }
        }
    }
}

/// Who last wrote the `len` bytes at `addr`, as memory was after record
/// `at`, and back through the copies that brought them there.
fn origin(addr: u32, len: u32, at: usize) -> String {
    let m = machine();
    m.view(|v| {
        let way = v.origin(addr, len, at, 8);
        if way.is_empty() {
            return "not written in the recording".into();
        }
        let place = |w: &ksee_core::view::Write| w.at.as_ref().map_or("?".into(), |l| format!("{} {}:{}", l.func, l.file, l.line));
        let mut out = vec![format!("written at {}, by {}; was {}", place(&way[0]), way[0].task.named(), way[0].old)];
        out.extend(way.iter().skip(1).map(|w| format!("  ← copied from {:#x}, written at {}", w.addr, place(w))));
        if let Some(from) = way.last().and_then(|w| w.from) {
            out.push(format!("  ← copied from {from:#x}, not written in the recording"));
        }
        out.join("\n")
    })
}

/// A value's row, and, opened, its members' (read as memory was at record
/// `at`); hovered, who last wrote it.
#[component]
pub fn Value(o: Object, path: String, depth: usize, at: usize) -> Element {
    let mut open = use_context::<Signal<BTreeSet<String>>>();
    let mut wrote = use_signal(String::new);
    let is_open = open.read().contains(&path);
    let can = o.children.is_some() || o.open.is_some();
    let kids = if is_open {
        machine().at_moment(at, |v, mem| o.members(&v.tables.types, mem))
    } else {
        None
    };
    let p = path.clone();
    let value = o.shown();
    let (addr, size) = (o.addr, o.size.clamp(1, 8));
    rsx! {
        // (One that opens: a click anywhere on its row.)
        tr { class: format!("{}{}", if o.missing.is_some() { "none" } else { "" }, if can { " opens" } else { "" }),
            onclick: move |_| if can { let mut s = open.write(); if !s.remove(&p) { s.insert(p.clone()); } },
            td { class: "fname", style: "padding-left: {depth * 14}px",
                if can { span { class: "tw", if is_open { "▾ " } else { "▸ " } } }
                "{o.name}"
            }
            td { class: "fval", title: "{wrote}",
                onmouseenter: move |_| if addr != 0 { wrote.set(origin(addr, size, at)) },
                "{value}" }
            td { class: "ftype", "{o.ty}" }
        }
        for c in kids.unwrap_or_default() {
            Value { o: c.clone(), path: format!("{path}.{}", c.name), depth: depth + 1, at }
        }
    }
}
