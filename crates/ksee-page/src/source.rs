//! The source of the frame shown (the kernel's, as the build copied it to
//! the site's src/), colored (highlight.rs): the whole file, scrolling (only
//! the lines in view made: a line's height is fixed), the function's lines
//! edged and the one it runs marked, in the middle, with its values under it as they are before it runs;
//! the lines that ran in the call selected (hover a line's number: how
//! often), each with what it stored so far (a click: that write); a name's
//! value at the moment shown, hovered (Ctrl+click: its definition). A
//! click on a line's number runs to its next run (Shift: its last).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use dioxus::html::geometry::PixelsVector2D;
use dioxus::prelude::*;
use ksee_core::view::{Frame, Object};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use crate::highlight::{self, Kind, Piece};
use crate::machine;
use crate::state::{Derived, Ui};

/// A file: its lines, as text and as pieces.
pub struct File {
    pub text: Vec<String>,
    pub pieces: Vec<Vec<Piece>>,
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

/// What the source shows instead of the cursor's place, until it moves: a
/// definition (Ctrl+click), file and line.
#[derive(Clone, PartialEq)]
struct Seen {
    file: String,
    line: u32,
}

#[component]
pub fn Source() -> Element {
    let d = use_context::<Derived>();
    let mut seen = use_signal(|| None::<Seen>);
    // (The cursor moved: back to its place.)
    use_effect(move || {
        let _ = d.moment.read();
        seen.set(None);
    });
    let Some(f) = crate::stack::shown() else { return rsx! { div { class: "source" } } };
    let (Some(path), Some(line)) = (f.file.clone(), f.line) else { return rsx! { div { class: "source" } } };
    let (path, line, def_of) = match seen() {
        Some(s) => (s.file, s.line, true),
        None => (path, line, false),
    };
    // (The function's lines, where its file has them.)
    let def = machine().view(|v| v.definition(&f.func)).filter(|d| d.file == path && d.line <= line && !def_of);
    let (from, to) = def.map_or((line, line), |d| (d.line, d.last.unwrap_or(line).max(line)));
    let i = crate::stack::moment();
    rsx! {
        div { class: "source",
            div { class: "srcbar mono",
                span { "{path}" }
                if !def_of { span { " · {f.func}" } }
                if def_of { a { onclick: move |_| seen.set(None), "back to the cursor" } }
            }
            Code { path: path.clone(), from, to, line, here: !def_of, frame: f, i, seen }
        }
    }
}

/// What the call selected did on each line of `file`: the lines that ran,
/// how often; what each stored, its note and the write's record.
fn ran(ui: Ui, file: &str) -> (BTreeMap<u32, u64>, BTreeMap<u32, (String, String, usize)>) {
    let m = machine();
    let k = (ui.call)();
    let counts = m.view(|v| v.coverage(file, k)).into_iter().map(|c| (c.line, c.count)).collect();
    let cur = crate::stack::moment();
    let notes = ui.syscall().map(|s| m.view(|v| v.notes(s.first, s.last.unwrap_or(m.records().saturating_sub(1)), file, cur)))
        .unwrap_or_default().into_iter().map(|n| (n.line, (n.short, n.full, n.i))).collect();
    (counts, notes)
}

/// The value of expression `e` (a name, then fields: `file->f_pos`) at
/// record `i`: an argument or local of frame `f`, else a global.
fn value_of(e: &str, f: &Frame, i: usize) -> Option<Object> {
    let path: Vec<&str> = e.split("->").flat_map(|p| p.split('.')).collect();
    let m = machine();
    m.at_moment(i, |v, mem| {
        let mut o = f.args.iter().flatten().chain(f.locals.iter().flatten()).find(|o| o.name == path[0]).cloned()
            .or_else(|| v.tables.types.globals.get(path[0]).map(|&(a, ty)| v.tables.types.object(mem, path[0], a, ty, 1)))?;
        for field in &path[1..] {
            let kids = o.children.clone().or_else(|| o.open.and_then(|t| v.tables.types.object(mem, &o.name, t.addr, t.ty, 1).children))?;
            o = kids.into_iter().find(|c| c.name == *field)?;
        }
        Some(o)
    })
}

/// The tip over a name: its value.
#[derive(Clone, PartialEq)]
struct Tip {
    x: f64,
    y: f64,
    text: String,
}

/// A line's height (px), and how many lines are made past the view.
const LINE: f64 = 17.0;
const MARGIN: usize = 30;

/// File `path`, scrolling: line `line` marked (`here`: the cursor's, with
/// its values under it, and kept in the middle), the function's `from..=to`
/// edged.
#[component]
fn Code(path: String, from: u32, to: u32, line: u32, here: bool, frame: Frame, i: usize, seen: Signal<Option<Seen>>) -> Element {
    let ui = use_context::<Ui>();
    let p = path.clone();
    let text = use_resource(use_reactive!(|(p,)| file(p)));
    let mut tip = use_signal(|| None::<Tip>);
    let mut top = use_signal(|| 0.0f64);
    let mut view = use_signal(|| 600.0f64);
    let mut handle = use_signal(|| None::<std::rc::Rc<MountedData>>);
    // (The marked line in the middle, as it changes, or the file, once it is loaded.)
    let loaded = text.read().clone().flatten().map(|f| Rc::as_ptr(&f) as usize);
    use_effect(use_reactive!(|(line, loaded)| {
        let _ = loaded;
        if let Some(h) = handle() {
            let y = ((line as f64 - 1.0) * LINE - view() / 2.0).max(0.0);
            spawn(async move { let _ = h.scroll(PixelsVector2D::new(0.0, y), ScrollBehavior::Instant).await; });
        }
    }));
    let Some(src) = text.read().clone().flatten() else { return rsx! { div { class: "scroll", p { class: "dim pad", "…" } } } };
    let (counts, notes) = ran(ui, &path);
    let total = src.text.len();
    // (The cursor's line: the frame's values its names have, as they are before it runs.)
    let values: Vec<String> = if here && line >= 1 && (line as usize) <= total {
        let words: Vec<&str> = src.pieces[line as usize - 1].iter().filter(|p| p.kind == Kind::Name).map(|p| p.text.as_str()).collect();
        frame.args.iter().flatten().chain(frame.locals.iter().flatten()).filter(|o| words.contains(&o.name.as_str()))
            .map(|o| format!("{} = {}", o.name, o.value.clone().unwrap_or_else(|| "{…}".into()))).collect()
    } else { Vec::new() };
    let extra = if values.is_empty() { 0 } else { 1 };
    let names: Vec<String> = frame.args.iter().flatten().chain(frame.locals.iter().flatten()).map(|o| o.name.clone()).collect();
    // (The lines in view, and a margin: the rest, spacers.)
    let first = ((top() / LINE) as usize).saturating_sub(MARGIN).min(total);
    let last = ((((top() + view()) / LINE) as usize) + MARGIN).min(total);
    let below = (total - last) as f64 * LINE + if (line as usize) > last { extra as f64 * LINE } else { 0.0 };
    let above = first as f64 * LINE + if (line as usize) <= first && line > 0 { extra as f64 * LINE } else { 0.0 };
    rsx! {
        div { class: "scroll",
            onmounted: move |e| async move {
                let h = e.data();
                if let Ok(r) = h.get_client_rect().await { view.set(r.size.height); }
                let y = ((line as f64 - 1.0) * LINE - view() / 2.0).max(0.0);
                let _ = h.scroll(PixelsVector2D::new(0.0, y), ScrollBehavior::Instant).await;
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
                                    onclick: move |e| {
                                        let dir = if e.modifiers().shift() { -1 } else { 1 };
                                        if let Some(j) = machine().run_of(&file, n, i, dir) { ui.go_record(j, None); }
                                    },
                                    "{n}" }
                                for (k , p) in pieces.iter().enumerate() {
                                    if p.kind == Kind::Name {
                                        {
                                            let e = highlight::expr(&pieces, k);
                                            let has = names.contains(&e.split(['-', '.']).next().unwrap_or("").to_string());
                                            let frame = frame.clone();
                                            let word = p.text.clone();
                                            rsx! {
                                                span { class: if has { "nm val" } else { "nm" },
                                                    onmouseenter: move |ev| {
                                                        let pt = ev.client_coordinates();
                                                        tip.set(value_of(&e, &frame, i).map(|o| Tip { x: pt.x, y: pt.y, text: format!("{e} = {}", o.value.unwrap_or_else(|| "{…}".into())) }));
                                                    },
                                                    onclick: move |ev| if ev.modifiers().ctrl() || ev.modifiers().meta() {
                                                        // (Its definition: a function, a type, a global; a member, of the one struct with it.)
                                                        let d = machine().view(|v| v.definition(&word).or_else(|| v.member_definition(None, &word)));
                                                        if let Some(d) = d { seen.set(Some(Seen { file: d.file, line: d.line })); }
                                                    },
                                                    "{p.text}"
                                                }
                                            }
                                        }
                                    } else {
                                        span { class: p.kind.class(), "{p.text}" }
                                    }
                                }
                                if let Some((short, full, at)) = note {
                                    a { class: "stored", title: "{full}", onclick: move |_| ui.go_record(at, None), "{short}" }
                                }
                            }
                            if is_here && !values.is_empty() {
                                div { key: "{n}:values", class: "values", span { class: "no" } "{values.join(\"   \")}" }
                            }
                        }
                    }
                }
                div { style: "height: {below}px" }
            }
        }
        if let Some(t) = tip() {
            div { class: "tip", style: "left: {t.x + 12.0}px; top: {t.y + 14.0}px", "{t.text}" }
        }
    }
}
