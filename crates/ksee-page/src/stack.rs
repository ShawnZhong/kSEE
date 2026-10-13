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

/// A move of the moment read. Next: the line that runs next (in time: in a
/// loop, its next turn), in the function you read (the frame chosen; else
/// the innermost), over what it calls, on in its caller once it returns.
/// In: up the stack, the frame above chosen; at the innermost, the very
/// next line (into the call its line makes, if it makes one). Back: the
/// way the last step on came (Ui::every), back: Next's, or In's, the very
/// line before, deeper maybe. Out: down the stack, the caller chosen, at
/// its call. (A step reads the innermost frame it lands in: the frame
/// chosen is one the cursor is not in yet.)
#[derive(Clone, Copy)]
pub enum Move {
    Back,
    Next,
    In,
    Out,
}

/// The steps: each its key (the page's, see listen), its button's label
/// and title. (Up and down: the stack, as the Stack and the chart have it,
/// the callers below.)
const MOVES: [(Move, &str, &str, &str); 4] = [
    (Move::Back, "ArrowLeft", "← Back", "Back in time, the way the last step came: after Next, the line before in the function you read; after In, the very line before (←)"),
    (Move::Next, "ArrowRight", "→ Next", "On in time: the line that runs next, in the function you read, over its calls (→)"),
    (Move::In, "ArrowUp", "↑ In", "The very next line, into the call this line makes, if any (↑; with a caller's frame chosen: the frame above)"),
    (Move::Out, "ArrowDown", "↓ Out", "Out to the caller, at its call (↓)"),
];

/// The moment shown (hovered, else the cursor) moved by `m`, the cursor
/// pinned there; past the run's lines, into its task's run after it (or
/// before): its next syscall, or what it did between.
pub fn walk(ui: Ui, d: Derived, m: Move) {
    let Some(at) = d.shown.peek().clone() else { return };
    let k = at.run;
    let lines = crate::state::run_lines(k);
    let (n, f) = (d.stack.peek().len(), *d.frame.peek());
    let depth = d.stack.peek().get(f).map_or(0, |f| f.depth);
    let read = |f: usize| Some(Moment { level: (f > 0).then(|| n - 1 - f), ..at.clone() });
    let dir = match m {
        Move::Out if f + 1 < n => return ui.commit(read(f + 1)),
        Move::In if f > 0 => return ui.commit(read(f - 1)),
        Move::Out => return,
        Move::Back | Move::Next | Move::In => {
            let mut every = ui.every;
            let dir = match m {
                Move::Back => -1,
                _ => (every.set(matches!(m, Move::In)), 1).1,
            };
            if let Some(j) = step(&lines, at.line, dir, (!*every.peek()).then_some(depth)) {
                return ui.commit(Some(Moment { run: k, line: j, level: None }));
            }
            dir
        }
    };
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
            ui.go(crate::state::run_lines(next).len().saturating_sub(1));
        }
    }
}

/// Under the strip: the moves (Move), with their keys as their icons: →
/// Next in the function read and ← Back the way it came, ↑ In and ↓ Out
/// on the stack; and where the cursor is: the frame you read (chosen on
/// the Stack or the chart; else the innermost).
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
            for (m, _, label, title) in MOVES {
                button { title, onclick: move |_| walk(ui, d, m), "{label}" }
            }
            span { class: "where mono dim", "{place}{turn}{returned}" }
        }
    }
}

/// The page's keys, anywhere on it: one listener on the window, each its
/// button's arrow (see Move). Not in text fields (the lesson's lines,
/// find): arrows move the cursor there.
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
            // (Once the kernel is up.)
            let Some(&(m, ..)) = MOVES.iter().find(|x| x.1 == e.key()) else { return };
            if matches!(*ui.kernel.peek(), Some(Ok(()))) {
                walk(ui, d, m);
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
            td { class: "fval", title: if let Some(m) = o.missing { m.why().to_string() } else { wrote() },
                onmouseenter: move |_| if addr != 0 { wrote.set(origin(addr, size, at)) },
                "{value}" }
            td { class: "ftype", "{o.ty}" }
        }
        for c in kids.unwrap_or_default() {
            Value { o: c.clone(), path: format!("{path}.{}", c.name), depth: depth + 1, at }
        }
    }
}
