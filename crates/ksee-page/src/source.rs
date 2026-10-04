//! The source of the frame shown (the kernel's, as the build copied it to
//! the site's src/), colored (highlight.rs): the whole file, scrolling (only
//! the lines in view made: a line's height is fixed), the function's lines
//! edged and the one it runs marked, with its values at its end as they are before it runs;
//! the lines that ran in the call selected (hover a line's number: how
//! often), each with what it stored so far (a click: that write); a name's
//! value at the moment shown, hovered (Ctrl+click: its definition). A
//! click on a line's number runs to its next run (Shift: its last).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use dioxus::html::geometry::PixelsVector2D;
use dioxus::prelude::*;
use ksee_core::view::{Frame, Object, SyscallSummary};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use ksee_core::highlight::{self, Kind, Piece};
use crate::machine;
use crate::state::{Derived, Ui};

/// A file: its lines, as text and as pieces.
pub struct File {
    pub text: Vec<String>,
    pub pieces: Vec<Vec<Piece>>,
}

/// The page's CSS class for a semantic source piece.
fn kind_class(kind: Kind) -> &'static str {
    match kind {
        Kind::Plain => "",
        Kind::Keyword => "kw",
        Kind::Type => "ty",
        Kind::Str => "str",
        Kind::Esc => "esc",
        Kind::Comment => "com",
        Kind::Number => "num",
        Kind::Pre => "pp",
        Kind::Name => "nm",
        Kind::Func => "fn",
        Kind::Field => "fld",
        Kind::Macro => "mac",
        Kind::Bool => "bl",
    }
}

thread_local! {
    static FILES: RefCell<BTreeMap<String, Rc<File>>> = const { RefCell::new(BTreeMap::new()) };
}

/// Source file `file`, fetched and colored once.
async fn file(file: String) -> Option<Rc<File>> {
    if let Some(f) = FILES.with_borrow(|f| f.get(&file).cloned()) {
        return Some(f);
    }
    let w = web_sys::window()?;
    let r: web_sys::Response = JsFuture::from(w.fetch_with_str(&format!("src/{file}"))).await.ok()?.dyn_into().ok()?;
    if !r.ok() {
        return None;
    }
    let text: Vec<String> = JsFuture::from(r.text().ok()?).await.ok()?.as_string()?.lines().map(String::from).collect();
    let f = Rc::new(File { pieces: highlight::file(&text), text });
    FILES.with_borrow_mut(|fs| fs.insert(file, f.clone()));
    Some(f)
}

#[component]
pub fn Source() -> Element {
    let d = use_context::<Derived>();
    let ui = use_context::<Ui>();
    // (A definition shown until the cursor moves: Ui::commit.)
    let seen = ui.seen;
    let Some(f) = crate::stack::shown() else { return rsx! { div { class: "source" } } };
    let (Some(path), Some(line)) = (f.file.clone(), f.line) else { return rsx! { div { class: "source" } } };
    let (path, line, def_of) = match seen() {
        Some((file, line)) => (file, line, true),
        None => (path, line, false),
    };
    // (The function's lines, where its file has them.)
    let def = machine().view(|v| v.definition(&f.func)).filter(|d| d.file == path && d.line <= line && !def_of);
    let sealed = def.as_ref().is_some_and(|d| d.sealed);
    let (from, to) = def.map_or((line, line), |d| (d.line, d.last.unwrap_or(line).max(line)));
    let at = d.line.cloned().map_or(0, |l| l.at);
    rsx! {
        div { class: "source",
            div { class: "srcbar mono",
                span { "{path}" }
                if !def_of { span { " · {f.func}" } }
                if sealed { span { title: "it loops, writes nothing and calls nothing: only its call and its return are recorded, not each turn", " · sealed: a box, its turns not recorded" } }
                if def_of { a { onclick: move |_| ui.show_definition(None), "back to the cursor" } }
            }
            Code { path: path.clone(), from, to, line, here: !def_of, frame: f, at, seen }
        }
    }
}

/// What the call selected did on each line of `file`: the lines that ran,
/// how often; what each stored, its note and the write's record.
fn ran(ui: Ui, syscall: Option<SyscallSummary>, file: &str, at: usize) -> (BTreeMap<u32, u64>, BTreeMap<u32, (String, String, usize)>) {
    let m = machine();
    let k = ui.call();
    let counts = m.view(|v| v.coverage(file, k));
    let notes = syscall.map(|s| m.view(|v| v.notes(s.first, s.last, file, at)))
        .unwrap_or_default().into_iter().map(|n| (n.line, (n.short, n.full, n.i))).collect();
    (counts, notes)
}

/// The value of expression `e` (a name, then fields: `file->f_pos`) at
/// record `at`: an argument or local of frame `f`, else a global.
fn value_of(e: &str, f: &Frame, at: usize) -> Option<Object> {
    let path: Vec<&str> = e.split("->").flat_map(|p| p.split('.')).collect();
    let m = machine();
    m.at_moment(at, |v, mem| {
        let mut o = f.args.iter().chain(&f.locals).find(|o| o.name == path[0]).cloned()
            .or_else(|| v.tables.types.globals.get(path[0]).map(|&(a, ty)| v.tables.types.object(mem, path[0], a, ty, 1)))?;
        let kids = |o: &Object| o.children.clone().or_else(|| o.open.and_then(|t| v.tables.types.object(mem, &o.name, t.addr, t.ty, 1).children));
        for field in &path[1..] {
            o = kids(&o)?.into_iter().find(|c| c.name == *field)?;
        }
        // (Its members, or what it points to: the tip shows them.)
        o.children = kids(&o);
        Some(o)
    })
}

/// The tip over a name: its value, then its members' (or what it points to).
#[derive(Clone, PartialEq)]
struct Tip {
    x: f64,
    y: f64,
    lines: Vec<String>,
}

/// What the tip over expression `e` says: its value, then its members'
/// (the first 16).
fn said(e: &str, o: &Object) -> Vec<String> {
    let value = |o: &Object| o.value.clone().unwrap_or_else(|| "{…}".into());
    let kids = o.children.as_deref().unwrap_or_default();
    std::iter::once(format!("{e} = {}", value(o)))
        .chain(kids.iter().take(16).map(|c| format!("{} = {}", c.name, value(c))))
        .chain((kids.len() > 16).then(|| format!("… {} more", kids.len() - 16)))
        .collect()
}

/// A line's height (px), and how many lines are made past the view.
const LINE: f64 = 17.0;
const MARGIN: usize = 30;
/// The lines kept between the marked line and the view's edges before it scrolls.
const KEEP: f64 = 3.0;

/// Where to scroll for line `line` to be in view, the view at `top`, `view`
/// high: nowhere while it is clear of the edges; else
/// with it a third of the way down, the lines after it in view.
fn place(line: u32, top: f64, view: f64) -> Option<f64> {
    let y = (line as f64 - 1.0) * LINE;
    let clear = y >= top + KEEP * LINE && y + (KEEP + 1.0) * LINE <= top + view;
    (!clear).then(|| (y - view / 3.0).max(0.0))
}

/// File `path`, scrolling: line `line` marked (`here`: the cursor's, with
/// its values under it, and kept in view), the function's `from..=to`
/// edged.
#[component]
fn Code(path: String, from: u32, to: u32, line: u32, here: bool, frame: Frame, at: usize, seen: Signal<Option<(String, u32)>>) -> Element {
    let ui = use_context::<Ui>();
    (ui.snapshot)();
    let p = path.clone();
    let text = use_resource(use_reactive!(|(p,)| file(p)));
    let mut tip = use_signal(|| None::<Tip>);
    let mut top = use_signal(|| 0.0f64);
    let mut view = use_signal(|| 600.0f64);
    let mut handle = use_signal(|| None::<std::rc::Rc<MountedData>>);
    // (The marked line in view, as it changes: the view still while it is.
    // Another file, once loaded: its line placed afresh, not by where the
    // last file was scrolled.)
    let loaded = text.read().clone().flatten().map(|f| Rc::as_ptr(&f) as usize);
    let mut placed = use_signal(|| None::<usize>);
    use_effect(use_reactive!(|(line, loaded)| {
        let fresh = loaded.is_some() && *placed.peek() != loaded;
        if fresh {
            placed.set(loaded);
        }
        // (Read, not followed: a scroll is no reason to move it again.)
        let top = if fresh { f64::NEG_INFINITY } else { *top.peek() };
        if let (Some(h), Some(y)) = (handle.peek().clone(), place(line, top, *view.peek())) {
            spawn(async move { let _ = h.scroll(PixelsVector2D::new(0.0, y), ScrollBehavior::Instant).await; });
        }
    }));
    let src = match text.read().clone() {
        None => return rsx! { div { class: "scroll", p { class: "dim pad", "loading {path}…" } } },
        Some(None) => return rsx! { div { class: "scroll", p { class: "dim pad", "{path}: not among the site's sources" } } },
        Some(Some(src)) => src,
    };
    let d = use_context::<Derived>();
    let (counts, notes) = ran(ui, d.syscall.cloned(), &path, at);
    let total = src.text.len();
    // (The cursor's line, at its end: the frame's values its names have, as
    // they are before it runs; what it compared, which way its branch went;
    // the call it made, once returned, what it returned.)
    let shown = d.line.cloned();
    let values: Vec<String> = if here && line >= 1 && (line as usize) <= total {
        let words: Vec<&str> = src.pieces[line as usize - 1].iter().filter(|p| p.kind.is_ident()).map(|p| p.text.as_str()).collect();
        // (A call made on this line or this function's line before it, returned by now.)
        let before = d.shown.read().as_ref().and_then(|m| {
            let lines = d.lines.read();
            let depth = lines.get(m.line)?.depth;
            lines[..m.line].iter().rev().find(|l| l.depth <= depth).filter(|l| l.depth == depth).cloned()
        });
        let returned = shown.as_ref().and_then(|l| [Some(l), before.as_ref()].into_iter().flatten().find_map(|c| c.called.as_ref())
            .filter(|c| c.at <= l.at).map(|c| format!("{} → {}", c.func, machine().tables().types.result(c.value))));
        frame.args.iter().chain(&frame.locals)
            .filter(|o| words.contains(&o.name.as_str()) && o.none != Some(ksee_core::view::Missing::NotRecorded))
            .map(|o| format!("{} = {}", o.name, o.value.clone().unwrap_or_else(|| "{…}".into())))
            .chain(shown.map(|l| l.tests).unwrap_or_default()).chain(returned).collect()
    } else { Vec::new() };
    let names: Vec<String> = frame.args.iter().chain(&frame.locals).map(|o| o.name.clone()).collect();
    // (The lines in view, and a margin: the rest, spacers.)
    let first = ((top() / LINE) as usize).saturating_sub(MARGIN).min(total);
    let last = ((((top() + view()) / LINE) as usize) + MARGIN).min(total);
    let (above, below) = (first as f64 * LINE, (total - last) as f64 * LINE);
    rsx! {
        div { class: "scroll",
            onmounted: move |e| async move {
                let h = e.data();
                if let Ok(r) = h.get_client_rect().await { view.set(r.size.height); }
                if let Some(y) = place(line, 0.0, view()) {
                    let _ = h.scroll(PixelsVector2D::new(0.0, y), ScrollBehavior::Instant).await;
                }
                handle.set(Some(h));
            },
            onscroll: move |e| { top.set(e.scroll_top()); view.set(e.client_height() as f64); },
            onmouseleave: move |_| tip.set(None),
            div { class: "code mono",
                div { style: "height: {above}px" }
                for n in (first as u32 + 1)..=(last as u32) {
                    {
                        let pieces = src.pieces[n as usize - 1].clone();
                        let is_here = here && n == line;
                        let infn = n >= from && n <= to;
                        let class = format!("ln{}{}{}", if is_here { " now" } else if counts.contains_key(&n) { " ran" } else { "" }, if infn { " infn" } else { "" }, if n == line && !here { " seen" } else { "" });
                        let count = counts.get(&n).map_or("did not run in the call selected".to_string(), |c| format!("ran {c}×; click: run to its next run (Shift: its last)"));
                        let note = notes.get(&n).cloned();
                        let frame = frame.clone();
                        let names = names.clone();
                        let file = path.clone();
                        rsx! {
                            div { key: "{n}", class,
                                span { class: "no", title: "{count}",
                                    onclick: move |e| ui.run_to(&file, n, if e.modifiers().shift() { -1 } else { 1 }),
                                    "{n}" }
                                for (k , p) in pieces.iter().enumerate() {
                                    if p.kind.is_ident() {
                                        {
                                            let e = highlight::expr(&pieces, k);
                                            let has = names.contains(&e.split(['-', '.']).next().unwrap_or("").to_string());
                                            let frame = frame.clone();
                                            let word = p.text.clone();
                                            rsx! {
                                                span { class: if has { format!("{} val", kind_class(p.kind)) } else { kind_class(p.kind).to_string() },
                                                    onmouseenter: move |ev| {
                                                        let pt = ev.client_coordinates();
                                                        tip.set(value_of(&e, &frame, at).map(|o| Tip { x: pt.x, y: pt.y, lines: said(&e, &o) }));
                                                    },
                                                    onclick: move |ev| if ev.modifiers().ctrl() || ev.modifiers().meta() {
                                                        // (Its definition: a function, a type, a global; a member, of the one struct with it.)
                                                        let d = machine().view(|v| v.definition(&word).or_else(|| v.member_definition(&word)));
                                                        if let Some(d) = d { seen.set(Some((d.file, d.line))); }
                                                    },
                                                    "{p.text}"
                                                }
                                            }
                                        }
                                    } else {
                                        span { class: kind_class(p.kind), "{p.text}" }
                                    }
                                }
                                if is_here && !values.is_empty() {
                                    span { class: "values", title: "{values.join(\"\n\")}", "{values.join(\"   \")}" }
                                }
                                if let Some((short, full, at)) = note {
                                    a { class: "stored", title: "{full}", onclick: move |_| ui.go_record(d, at, None), "{short}" }
                                }
                            }
                        }
                    }
                }
                div { style: "height: {below}px" }
            }
        }
        if let Some(t) = tip() {
            div { class: "tip", style: "left: {t.x + 12.0}px; top: {t.y + 14.0}px",
                for (k , l) in t.lines.into_iter().enumerate() { div { class: if k > 0 { "f" } else { "" }, "{l}" } }
            }
        }
    }
}
