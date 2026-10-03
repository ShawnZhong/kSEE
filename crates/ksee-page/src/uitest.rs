//! The tests, run in the page built with them (target/test/site, in
//! headless Chromium: `ksee test`, crates/ksee-cli/src/chrome.rs): the
//! kernel's (selftest.rs), and the page's: what a user does, through its
//! DOM, and what it shows then. Each returns a report, or why it failed.

use js_sys::Promise;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Document, Element, Event, EventInit, HtmlElement, HtmlInputElement, HtmlTextAreaElement, KeyboardEvent, KeyboardEventInit,
              MouseEvent, MouseEventInit};

/// The test the URL names (`#…test=NAME`, set by chrome.rs), started: its
/// outcome POSTed to the server's /result, `ok` or `fail` then its log.
pub fn start() -> Option<String> {
    let hash = web_sys::window()?.location().hash().ok()?;
    let name = hash.trim_start_matches('#').split('&').find_map(|kv| kv.strip_prefix("test="))?.to_string();
    let run = name.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let body = match run_test(run).await {
            Ok(report) => format!("ok\n{report}"),
            Err(e) => format!("fail\n{}", e.as_string().unwrap_or_default()),
        };
        let init = web_sys::RequestInit::new();
        init.set_method("POST");
        init.set_body(&body.into());
        let _ = JsFuture::from(web_sys::window().unwrap().fetch_with_str_and_init("/result", &init)).await;
    });
    Some(name)
}

/// Test `name` (kernel, read, back, pipe, phone; the page each needs
/// opened: chrome.rs): its report, or an error saying what failed.
async fn run_test(name: String) -> Result<String, JsValue> {
    let r = match name.as_str() {
        "kernel" => kernel().await,
        "read" => read().await,
        "back" => back().await,
        "pipe" => pipe().await,
        "phone" => phone().await,
        _ => Err(format!("no test {name}")),
    };
    r.map_err(|e| JsValue::from_str(&e))
}

type Check = Result<String, String>;

fn doc() -> Document {
    web_sys::window().and_then(|w| w.document()).expect("a document")
}

fn all(sel: &str) -> Vec<Element> {
    let list = doc().query_selector_all(sel).expect("a selector");
    (0..list.length()).filter_map(|k| list.item(k)?.dyn_into().ok()).collect()
}

fn one(sel: &str) -> Option<Element> {
    doc().query_selector(sel).ok().flatten()
}

fn text(e: &Element) -> String {
    e.text_content().unwrap_or_default()
}

/// The first of `sel` whose text is `t`.
fn named(sel: &str, t: &str) -> Option<Element> {
    all(sel).into_iter().find(|e| text(e) == t)
}

async fn sleep(ms: i32) {
    let p = Promise::new(&mut |done, _| {
        let _ = web_sys::window().unwrap().set_timeout_with_callback_and_timeout_and_arguments_0(&done, ms);
    });
    let _ = JsFuture::from(p).await;
}

/// Wait until `f` holds, checking every 100 ms, `n` times at most.
async fn until(f: impl Fn() -> bool, n: usize) -> bool {
    for _ in 0..n {
        if f() {
            return true;
        }
        sleep(100).await;
    }
    f()
}

fn click(e: &Element) {
    if let Some(h) = e.dyn_ref::<HtmlElement>() {
        h.click();
    }
}

fn mouse(e: &Element, kind: &str, x: f64, y: f64, shift: bool) {
    let init = MouseEventInit::new();
    init.set_bubbles(true);
    init.set_client_x(x as i32);
    init.set_client_y(y as i32);
    init.set_shift_key(shift);
    let _ = e.dispatch_event(&MouseEvent::new_with_mouse_event_init_dict(kind, &init).unwrap());
}

fn key(e: &Element, k: &str) {
    let init = KeyboardEventInit::new();
    init.set_bubbles(true);
    init.set_key(k);
    let _ = e.dispatch_event(&KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init).unwrap());
}

fn input(e: &Element) {
    let init = EventInit::new();
    init.set_bubbles(true);
    let _ = e.dispatch_event(&Event::new_with_event_init_dict("input", &init).unwrap());
}

/// The program's lines (not the one that adds a line): their text and what each did.
fn cells() -> Vec<(String, String)> {
    all("ol.steps > li:not(.add) .cell").iter().map(|c| {
        let t = c.query_selector("textarea").ok().flatten().and_then(|t| t.dyn_into::<HtmlTextAreaElement>().ok()).map(|t| t.value());
        let r = c.query_selector(".ret").ok().flatten().map(|r| text(&r));
        (t.unwrap_or_default(), r.unwrap_or_default())
    }).collect()
}

/// The first `n` lines made, each with what it did.
fn done(n: usize) -> bool {
    let c = cells();
    c.len() >= n && c[..n].iter().all(|(_, r)| !r.is_empty() && r != "…")
}

/// Where the cursor is, as the transport says ("task · func file:line · in call"): "func file:line".
fn place() -> String {
    one(".transport .where").map(|w| text(&w)).and_then(|t| t.split(" · ").nth(1).map(String::from)).unwrap_or_default()
}

/// A click on a transport button (Shift held: over the calls).
fn press(label: &str, shift: bool) {
    if let Some(b) = named(".transport button", label) {
        mouse(&b, "click", 0.0, 0.0, shift);
    }
}

/// A line's text changed and Enter pressed.
fn write(ta: &Element, t: &str) {
    let Some(a) = ta.dyn_ref::<HtmlTextAreaElement>() else { return };
    let _ = a.focus();
    a.set_value(t);
    let _ = a.set_selection_range(t.len() as u32, t.len() as u32);
    input(ta);
    key(ta, "Enter");
}

fn fail(what: &str, detail: impl std::fmt::Debug) -> String {
    format!("{what}: {detail:?}")
}

/// Check `ok`, else fail saying `what` and `detail`.
fn check(ok: bool, what: &str, detail: impl std::fmt::Debug) -> Result<(), String> {
    if ok { Ok(()) } else { Err(fail(what, detail)) }
}

/// A tab chosen.
async fn tab(name: &str) {
    if let Some(t) = named(".tabs .tab", name) {
        click(&t);
    }
    sleep(300).await;
}

/// Wait for `f`, at most `n` checks; then check it.
async fn wait(f: impl Fn() -> bool, n: usize, what: &str, detail: impl Fn() -> String) -> Result<(), String> {
    let ok = until(f, n).await;
    check(ok, what, detail())
}

/// The kernel's tests (selftest.rs), on the Machine `mount` was given,
/// once booted (the page not mounted: #selftest).
async fn kernel() -> Check {
    crate::boot().await?;
    crate::selftest::self_test().await.map(|()| "passed".into())
}

/// The read example (#ex=read): its calls made, its subject (the read)
/// selected; its links followed; stepping into, over, out, and by key; the
/// chart (a click, find); Objects and Tasks; a line changed, a
/// try and back; the next lesson.
async fn read() -> Check {
    let mut r = Vec::new();
    wait(|| done(3) && !all(".chart .bar").is_empty(), 300, "the calls made", || format!("{:?}", cells())).await?;
    let selected = one(".cell.on textarea").and_then(|t| t.dyn_into::<HtmlTextAreaElement>().ok()).map(|t| t.value()).unwrap_or_default();
    check(selected.starts_with("read("), "the subject call selected", &selected)?;
    // (The source's file is fetched: drawn a moment later.)
    let drawn = || [all(".chart .bar").len(), all(".strip .run").len(), all(".source .ln").len(), all("tr.frame").len()];
    wait(|| drawn().iter().all(|&n| n > 0), 100, "drawn (bars, strip, source, frames)", || format!("{:?}", drawn())).await?;
    let drawn = drawn();
    r.push(format!("drawn {drawn:?}"));
    // (The links: each in its function, the source at its line.)
    for name in ["ksys_read", "vfs_read", "filemap_read"] {
        click(&named(".explain a", name).ok_or(fail("no link", name))?);
        let ok = until(|| place().starts_with(name) && one(".source .ln.now").is_some(), 50).await;
        check(ok, &format!("the link to {name}"), place())?;
        r.push(place());
    }
    // (Stepping: on a line, back, and over vfs_read's calls (no deeper).)
    click(&named(".explain a", "vfs_read").ok_or("no link to vfs_read")?);
    sleep(300).await;
    let at = place();
    press("▶", false);
    sleep(200).await;
    let into = place();
    press("◀", false);
    sleep(200).await;
    for _ in 0..3 {
        press("▶", true);
        sleep(150).await;
    }
    let over = place();
    check(into != at && over.starts_with("vfs_read"), "stepping", (&at, &into, &over))?;
    // (Keys, on the work area: → a line.)
    let work = one(".work").ok_or("no work area")?;
    let _ = work.dyn_ref::<HtmlElement>().map(|w| w.focus());
    let before = place();
    key(&work, "ArrowRight");
    sleep(200).await;
    check(place() != before, "→", (&before, place()))?;
    // (A click on a bar: the cursor there, that frame chosen.)
    let bar = all(".chart .bar").into_iter().find(|b| text(b).starts_with("vfs_read")).ok_or("no vfs_read bar")?;
    let b = bar.get_bounding_client_rect();
    mouse(&one(".chart").ok_or("no chart")?, "click", b.left() + b.width() / 2.0, b.top() + b.height() / 2.0, false);
    sleep(300).await;
    let chosen = one("tr.frame.on a").map(|a| text(&a)).unwrap_or_default();
    check(chosen == "vfs_read", "a click on a bar", (&chosen, place()))?;
    // (Find: its bars lit; Enter, the cursor in it.)
    let find = one(".find").ok_or("no find box")?;
    find.dyn_ref::<HtmlInputElement>().ok_or("find, not an input")?.set_value("copy_page_to_iter");
    input(&find);
    sleep(200).await;
    let found = all(".chart .bar.found").len();
    key(&find, "Enter");
    sleep(300).await;
    check(found > 0 && named("tr.frame a", "copy_page_to_iter").is_some(), "find", (found, place()))?;
    // (Objects: what the read changed, its file's position; Tasks: a task_struct opened.)
    tab("Objects").await;
    let objects: Vec<String> = all(".obj .title").iter().map(text).collect();
    check(named(".obj td", "f_pos").is_some(), "objects", &objects)?;
    r.push(format!("objects {objects:?}"));
    tab("Tasks").await;
    let rows = all(".pane tr").len();
    click(&one(".pane tr.frame a").ok_or("no task")?);
    sleep(300).await;
    let opened = all(".pane tr").len();
    check(opened > rows + 5 && named(".pane td", "comm").is_some(), "a task_struct opened", (rows, opened))?;
    tab("Stack").await;
    // (A line changed: the program made again; 4 bytes read.)
    write(all("ol.steps .cell textarea").get(2).ok_or("no third line")?, "read(fd, buf, 4)");
    let c = || cells().get(2).cloned().unwrap_or_default();
    wait(|| c() == ("read(fd, buf, 4)".into(), "= 4".into()), 300, "a line changed", || format!("{:?}", cells())).await?;
    let hash = web_sys::window().unwrap().location().hash().unwrap_or_default();
    check(hash.contains("&p="), "the URL: the program changed", &hash)?;
    // (Back to the lesson's program; a try: its program; and back.)
    click(&one(".ptitle a").ok_or("no reset")?);
    wait(|| c().1 == "= 18", 300, "back to the lesson's program", || format!("{:?}", cells())).await?;
    click(&named(".explain a", "read twice").ok_or("no try")?);
    wait(|| cells().len() == 5 && done(5), 300, "the try", || format!("{:?}", cells())).await?;
    let tried: Vec<String> = cells().into_iter().map(|c| c.1).collect();
    check(tried[3] == "= 0", "the try: its second read", &tried)?;
    click(&one(".ptitle a").ok_or("no reset")?);
    wait(|| cells().len() == 3 && done(3), 300, "back to the lesson's program", || format!("{:?}", cells())).await?;
    // (The next lesson.)
    let title = one("h1").map(|h| text(&h)).unwrap_or_default();
    click(&one(".topbar button[title='the next lesson']").ok_or("no next lesson")?);
    wait(|| one("h1").map(|h| text(&h)) != Some(title.clone()), 100, "the next lesson", || title.clone()).await?;
    Ok(r.join("; "))
}

/// The page opened with a program changed (the URL's p=): its lines made,
/// said to be a try, and the lesson's own program a click away.
async fn back() -> Check {
    wait(|| done(2), 300, "the program made", || format!("{:?}", cells())).await?;
    let note = one(".explain.dim").map(|n| text(&n)).unwrap_or_default();
    let c = cells();
    check(c.len() == 2 && c[1].1 == "= 9" && note.contains("not the lesson's own program") && one(".ptitle a").is_some(), "not back", (&c, &note))?;
    Ok(format!("{c:?}"))
}

/// The pipe example (two processes: the program, and a child sharing its
/// memory, with fds of its own): the child's read blocked, finished as the
/// program's write woke it; what each call changed; both tasks in the strip.
async fn pipe() -> Check {
    wait(|| done(7) && !all(".strip .run").is_empty(), 300, "the calls made", || format!("{:?}", cells())).await?;
    let c = cells();
    let changed: Vec<String> = all(".cell .more").iter().map(text).collect();
    // (The child's line selected: its task in the strip as the program's other.)
    let _ = all("ol.steps .cell textarea").get(5).ok_or("no sixth line")?.dyn_ref::<HtmlElement>().map(|t| t.focus());
    sleep(500).await;
    let mut strip: Vec<String> = all(".strip .run").iter().map(text).filter(|n| n.starts_with("prog(")).collect();
    strip.sort();
    strip.dedup();
    check(c[5].1 == "= 6 (during line 7)" && c[6].1 == "= 6" && changed.iter().any(|x| x == "fds = {3, 4}") && strip.len() >= 2,
          "the pipe", (&c, &changed, &strip))?;
    Ok(format!("{changed:?} {strip:?}"))
}

/// The read example on a phone's screen (390x844): the lesson, then the
/// chart, the source, the panels, one under another, the page never wider
/// than the screen; ▶ steps.
async fn phone() -> Check {
    wait(|| done(3) && !all(".chart .bar").is_empty(), 300, "the calls made", || format!("{:?}", cells())).await?;
    let top = |s: &str| one(s).map(|e| e.get_bounding_client_rect().top()).unwrap_or(f64::NAN);
    let order = top(".lesson") < top(".chart") && top(".chart") < top(".source") && top(".source") < top(".panels");
    let wide = doc().document_element().map(|e| e.scroll_width()).unwrap_or(0);
    let screen = web_sys::window().unwrap().inner_width().ok().and_then(|w| w.as_f64()).unwrap_or(0.0) as i32;
    check(order && wide <= screen, "the phone", (order, wide, screen))?;
    let at = place();
    press("▶", false);
    sleep(300).await;
    Ok(format!("{at} → {}", place()))
}
