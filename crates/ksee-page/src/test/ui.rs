//! Page behavior exercised through its rendered DOM.

use wasm_bindgen::JsCast;
use web_sys::{HtmlElement, HtmlInputElement, HtmlTextAreaElement};

use super::Check;
use super::dom::*;

/// The read example (read.html), a phase each, in order (each goes on
/// from where the one before left the page): the timeline's colors; its
/// calls made; its links and stepping; the timeline (hover, click, the
/// cross, a level kept); a name's tip in the source; the timeline's window
/// (find, pan, zoom, the overview); the editor's completions; the Memory
/// and Tasks tabs; the program changed, a try, the lessons before and after.
pub(super) async fn read() -> Check {
    let mut r = Vec::new();
    // (A failure says its phase.)
    let at = |phase: &'static str| move |e: String| format!("{phase}: {e}");
    colors().map_err(at("colors"))?;
    made(&mut r).await.map_err(at("made"))?;
    links_and_steps(&mut r).await.map_err(at("links and steps"))?;
    timeline(&mut r).await.map_err(at("timeline"))?;
    tip().await.map_err(at("tip"))?;
    window().await.map_err(at("window"))?;
    editor().await.map_err(at("editor"))?;
    panels(&mut r).await.map_err(at("panels"))?;
    program().await.map_err(at("program"))?;
    // (The memory the page took, kernel and all: it never shrinks.)
    r.push(format!("{} MiB of memory", core::arch::wasm32::memory_size(0) * 64 / 1024));
    Ok(r.join("; "))
}

/// The timeline's colors, by area.
fn colors() -> Result<(), String> {
    let colors = [(Some("fs/x.c"), false, "hsl(135 40% 74%)"), (Some("fs/x.c"), true, "hsl(135 50% 50%)"),
                  (Some("arch/x.c"), false, "hsl(215 18% 74%)"), (Some("include/x.h"), false, "hsl(210 5% 74%)"), (None, false, "hsl(0 0% 79%)")];
    if let Some((file, on, _)) = colors.iter().find(|(f, on, c)| crate::chart::call_color(*f, *on) != *c) {
        return Err(format!("the timeline's color for {file:?} (on: {on}) is {}", crate::chart::call_color(*file, *on)));
    }
    Ok(())
}

/// Its calls made, its subject (the read) selected, everything drawn.
async fn made(r: &mut Vec<String>) -> Result<(), String> {
    let width = || drew("width").and_then(|w| w.parse::<usize>().ok()).unwrap_or(0);
    wait(|| done(3) && width() > 0, 900, "the calls made",
         || format!("{:?}; {}", cells(), one(".pad.dim").map(|e| text(&e)).unwrap_or_default())).await?;
    let selected = one(".cell.on textarea").and_then(|t| t.dyn_into::<HtmlTextAreaElement>().ok()).map(|t| t.value()).unwrap_or_default();
    check(selected.starts_with("read("), "the subject call selected", &selected)?;
    // (The source's file is fetched: drawn a moment later.)
    let drawn = || [width(), drew("data-test-tasks").is_some() as usize, all(".source .ln").len(), all("tr.frame").len()];
    wait(|| drawn().iter().all(|&n| n > 0), 100, "drawn (bars, tasks, source, frames)", || format!("{:?}", drawn())).await?;
    let drawn = drawn();
    r.push(format!("drawn {drawn:?}"));
    Ok(())
}

/// The lesson's links, the global keys, stepping (on, back, over).
async fn links_and_steps(r: &mut Vec<String>) -> Result<(), String> {
    // (The links: each in its function, the source at its line.)
    for name in ["ksys_read", "vfs_read", "filemap_read"] {
        click(&named(".explain a", name).ok_or(fail("no link", name))?);
        let ok = until(|| place().starts_with(name) && one(".source .ln.now").is_some(), 100, 50).await;
        check(ok, &format!("the link to {name}"), place())?;
        r.push(place());
    }
    // (The global key listener: reset to a known cursor, then move it.)
    click(&named(".explain a", "vfs_read").ok_or("no link to vfs_read")?);
    settle().await;
    let cursor = || drew("data-test-now").unwrap_or_default();
    let before = cursor();
    global_key("ArrowRight");
    wait(|| cursor() != before, 20, "the global → key", || format!("{} at {}", cursor(), place())).await?;
    // (Stepping: Next, in the function read, vfs_read, over its calls,
    // each step somewhere else, and Back as far; In, every line, into the
    // calls, each somewhere else too, and Back the same way; Out, its
    // caller at its call, and Next on there.)
    click(&named(".explain a", "vfs_read").ok_or("no link to vfs_read")?);
    settle().await;
    let at = place();
    let mut over = vec![at.clone()];
    for _ in 0..8 {
        press("→ Next");
        settle().await;
        over.push(place());
    }
    for _ in 0..8 {
        press("← Back");
        settle().await;
    }
    let back = place();
    click(&named(".explain a", "vfs_read").ok_or("no link to vfs_read")?);
    settle().await;
    let mut next = vec![place()];
    for _ in 0..12 {
        press("↑ In");
        settle().await;
        next.push(place());
    }
    // (Back, after In: the way it came, every line, out of the calls.)
    let mut retraced = vec![place()];
    for _ in 0..12 {
        press("← Back");
        settle().await;
        retraced.push(place());
    }
    retraced.reverse();
    let moved = |p: &[String]| p.windows(2).all(|w| w[0] != w[1]);
    check(over.iter().all(|p| p.starts_with("vfs_read")) && moved(&over) && moved(&next) && next.iter().any(|p| !p.starts_with("vfs_read")) && back == at,
          "stepping: Next within the function, Back as far; In, into the calls; each step a move", (&over, &next, &back))?;
    check(retraced == next, "Back after In: the same lines, back", (&next, &retraced))?;
    click(&named(".explain a", "vfs_read").ok_or("no link to vfs_read")?);
    settle().await;
    press("↓ Out");
    settle().await;
    let caller = place();
    press("→ Next");
    settle().await;
    check(caller.starts_with("ksys_read") && place().starts_with("ksys_read") && place() != caller, "Out: the caller, at its call; Next, on in it", (&caller, place()))?;
    Ok(())
}

/// The timeline: hover (the moment shown) and click (pinned) on a bar, a level chosen
/// and kept, a drag after a click; how long a hover and a step take.
async fn timeline(r: &mut Vec<String>) -> Result<(), String> {
    // (Hover and click resolve the same canvas bar and moment.)
    let canvas = one("#timeline-canvas").ok_or("no timeline canvas")?;
    let (x, y) = bar(&canvas, "vfs_read").ok_or("no vfs_read canvas bar")?;
    let b = canvas.get_bounding_client_rect();
    let chart = one(".chart").ok_or("no chart")?;
    let (x, y) = (b.left() + x, b.top() + y);
    // (Hovered, the moment there shown everywhere, a bar saying what its
    // call was; left, the cursor's again, no tip.)
    let cross = || canvas.get_attribute("data-test-now");
    let chosen = || one("tr.frame.on a").map(|a| text(&a)).unwrap_or_default();
    let (before, at) = (cross(), place());
    pointer(&chart, "pointermove", x, y);
    settle().await;
    let tip = one(".tip").map(|t| text(&t)).unwrap_or_default();
    let (during, hovered, frame) = (cross(), place(), chosen());
    mouse(&chart, "mouseleave", x, y, false);
    settle().await;
    check(tip.contains("vfs_read") && during != before && hovered != at && frame == "vfs_read", "a hover: the moment shown, the bar's tip",
          (&tip, &before, &during, &at, &hovered, &frame))?;
    check(one(".tip").is_none() && cross() == before && place() == at, "left: the cursor's moment again", (cross(), &before, place(), &at))?;
    // (A click pins it: left, it stays, the bar's level chosen.)
    pointer(&chart, "pointermove", x, y);
    mouse(&chart, "click", x, y, false);
    mouse(&chart, "mouseleave", x, y, false);
    settle().await;
    let level = || canvas.get_attribute("data-test-level");
    check(chosen() == "vfs_read" && place() == hovered && level().is_some(),
          "a click pins the moment hovered, its bar's level chosen, a line across", (chosen(), place(), &hovered, level()))?;
    // (A click above the bars: no level chosen, the innermost frame shown.)
    mouse(&chart, "click", x, b.top() + 2.0, false);
    settle().await;
    let innermost = one(".stack tbody").is_some_and(|t| t.class_name() == "on");
    check(level().is_none() && innermost, "a click off the bars: no level chosen", (level(), innermost))?;
    mouse(&chart, "click", x, y, false);
    settle().await;
    // (Its frame chosen, stepping on in it: callees come and go above it,
    // and it, the same call, stays where it is.)
    let chosen = || one(".stack tr.frame.on").map(|r| (text(&r), r.get_bounding_client_rect().top().round()));
    let mut seen = vec![chosen()];
    for _ in 0..6 {
        press("→ Next");
        settle().await;
        seen.push(chosen());
    }
    let first = seen[0].clone();
    check(seen.iter().all(|c| c.as_ref().map(|c| &c.0) != first.as_ref().map(|c| &c.0) || *c == first),
          "the level chosen stays put as the cursor steps", &seen)?;
    mouse(&chart, "click", x, y, false);
    settle().await;
    // (Then, zoomed in (the wheel, up) and from the bar clicked, a drag:
    // the bars follow the pointer.)
    wheel(&chart, x, y, -600.0);
    settle().await;
    let (x, y) = bar(&canvas, "vfs_read").map(|(bx, by)| (b.left() + bx, b.top() + by)).ok_or("no vfs_read bar, zoomed in")?;
    let before = bar(&canvas, "vfs_read").map(|p| p.0);
    let win0 = one(".overview .win").and_then(|w| w.get_attribute("style"));
    pointer(&chart, "pointerdown", x, y);
    pointer(&chart, "pointermove", x + 60.0, y);
    settle().await;
    let during = bar(&canvas, "vfs_read").map(|p| p.0);
    pointer(&chart, "pointerup", x + 60.0, y);
    mouse(&chart, "click", x + 60.0, y, false);
    settle().await;
    let win1 = one(".overview .win").and_then(|w| w.get_attribute("style"));
    check(before.zip(during).is_some_and(|(a, b)| (b - a - 60.0).abs() < 1.0 || win0 != win1),
          "a drag after a click", (before, during, win0, win1))?;
    // (A click anywhere: the cursor there, the window still; on the bottom
    // row, then higher.)
    let win = || one(".overview .win").and_then(|w| w.get_attribute("style")).unwrap_or_default();
    let mut jumps = Vec::new();
    for row in [1.0, 3.0] {
        for k in 0..40 {
            let cx = b.left() + b.width() * (k as f64 + 0.5) / 40.0;
            let w0 = win();
            mouse(&chart, "click", cx, b.bottom() - row * 15.0 + 7.0, false);
            settle().await;
            let now = drew("data-test-now").and_then(|n| n.parse::<f64>().ok());
            if win() != w0 || now.is_none_or(|n| (n - (cx - b.left())).abs() > 2.0 * b.width() / 40.0) {
                jumps.push((row, k, now.map(|n| n.round()), (cx - b.left()).round(), w0 == win()));
            }
        }
    }
    check(jumps.is_empty(), "a click puts the cursor where it is, the window still", &jumps)?;
    mouse(&chart, "click", x, y, false);
    settle().await;
    // (Stepping past the window's edge: it slides along, the cursor just inside it, not a jump.)
    let edge = || drew("data-test-now").and_then(|n| n.parse::<f64>().ok());
    let mut seen = Vec::new();
    for _ in 0..200 {
        press("↑ In");
        settle().await;
        if let Some(n) = edge() { seen.push(n.round()) }
    }
    let leaps: Vec<_> = seen.windows(2).filter(|w| w[0] - w[1] > b.width() / 4.0).collect();
    check(leaps.is_empty(), "stepping slides the window along", (&leaps, seen.len()))?;
    // (Back in vfs_read, by the lesson's link: the window has moved since.)
    click(&named(".explain a", "vfs_read").ok_or("no link to vfs_read")?);
    settle().await;
    let (x, y) = bar(&canvas, "vfs_read").map(|(bx, by)| (b.left() + bx, b.top() + by)).ok_or("no vfs_read bar")?;
    mouse(&chart, "click", x, y, false);
    settle().await;
    // (How long a hover takes, the page drawn again: 30 moves across the chart.)
    let now = || web_sys::window().and_then(|w| w.performance()).map_or(0.0, |p| p.now());
    let t0 = now();
    for k in 0..30 {
        pointer(&chart, "pointermove", b.left() + b.width() * (k as f64 + 0.5) / 30.0, y);
        sleep(0).await;
    }
    let hover = (now() - t0) / 30.0;
    let t0 = now();
    for _ in 0..20 {
        press("↑ In");
        sleep(0).await;
    }
    let step = (now() - t0) / 20.0;
    r.push(format!("hover {hover:.1} ms, step {step:.1} ms"));
    // (Back in vfs_read for what follows: twenty steps leave it.)
    click(&named(".explain a", "vfs_read").ok_or("no link to vfs_read")?);
    settle().await;
    Ok(())
}

/// A name in the source, hovered: its value, opening.
async fn tip() -> Result<(), String> {
    // (A name in the source, hovered: its value as the Stack has one, open,
    // and a member opens further.)
    let name = all(".code .val").into_iter().find(|e| text(e) == "file").ok_or(format!("no `file` in the source, at {}", place()))?;
    let at = name.get_bounding_client_rect();
    mouse(&name, "mouseover", at.left() + 2.0, at.top() + 2.0, false);
    mouse(&name, "mouseenter", at.left() + 2.0, at.top() + 2.0, false);
    settle().await;
    let rows = || all(".tip.open tr").len();
    let before = rows();
    // (A member that opens: past the first row, the name itself.)
    let opens = all(".tip.open tr.opens").into_iter().nth(1).ok_or(format!("no member that opens in the tip ({before} rows)"))?;
    click(&opens);
    settle().await;
    check(before > 1 && rows() > before, "a name's tip: its members, one opening", (before, rows()))?;
    Ok(())
}

/// The timeline's window: find, pan, zoom out, the rows scrolled, the
/// overview clicked.
async fn window() -> Result<(), String> {
    // (From the call whole: ksys_read read, on the Stack, and zoomed to.)
    click(&named(".stack tr.frame a", "ksys_read").ok_or("no ksys_read on the Stack")?);
    settle().await;
    click(&named(".charthead a", "zoom to ksys_read").ok_or("no zoom to ksys_read")?);
    settle().await;
    let canvas = one("#timeline-canvas").ok_or("no timeline canvas")?;
    let chart = one(".chart").ok_or("no chart")?;
    let b = canvas.get_bounding_client_rect();
    let y = bar(&canvas, "vfs_read").map_or(b.bottom() - 20.0, |(_, by)| b.top() + by);
    // (Find: its bars lit; Enter, the cursor in it.)
    let find = one(".find").ok_or("no find box")?;
    find.dyn_ref::<HtmlInputElement>().ok_or("find, not an input")?.set_value("copy_page_to_iter");
    input(&find);
    settle().await;
    let found = drew("data-test-found").and_then(|n| n.parse().ok()).unwrap_or(0);
    key(&find, "Enter");
    settle().await;
    check(found > 0 && named("tr.frame a", "copy_page_to_iter").is_some(), "find", (found, place()))?;
    // (A drag pans, zoomed in: the bars follow the pointer, and the window
    // stays where it let go.)
    // (Zoomed in around vfs_read's bar, so that it stays in the window.)
    let mid = (b.left() + b.right()) / 2.0;
    let at = bar(&canvas, "vfs_read").map_or(mid, |(x, _)| b.left() + x);
    wheel(&chart, at, y, -800.0);
    settle().await;
    // (A call whose bar is whole in the window, and stays so dragged 80 px
    // left: its left edge, throughout.)
    let width = b.width();
    let call = bars(&canvas).into_iter().find(|c| c.4 > 100.0 && c.4 + c.5 < width - 1.0).map(|c| c.1).unwrap_or_default();
    let bar_x = || bars(&canvas).into_iter().find(|c| c.1 == call).map_or(f64::NAN, |c| c.4);
    let win = || one(".overview .win").and_then(|w| w.get_attribute("style")).unwrap_or_default();
    let (x0, w0) = (bar_x(), win());
    pointer(&chart, "pointerdown", at, y);
    pointer(&chart, "pointermove", at - 80.0, y);
    settle().await;
    let during = bar_x();
    // (What the drag brings in, drawn as it goes: the bars reach the right edge.)
    let reach = drew("data-test-reach").unwrap_or_default();
    let covered = reach.split_once(',').and_then(|(_, hi)| hi.parse::<f64>().ok()).is_some_and(|hi| hi >= b.width() - 2.0);
    pointer(&chart, "pointerup", at - 80.0, y);
    settle().await;
    check((x0 - 80.0 - during).abs() < 1.0 && win() != w0 && covered, "a drag pans, what it brings in drawn", (x0, during, w0, win(), reach))?;
    // (Zoomed out, the chart deeper or not: the bars stay where they were on
    // the screen, kept at the bottom.)
    let screen_y = || bar(&canvas, "ksys_read").map(|(_, y)| canvas.get_bounding_client_rect().top() + y);
    let y0 = screen_y();
    wheel(&chart, mid, y, 1600.0);
    settle().await;
    let y1 = screen_y();
    check(y0.zip(y1).is_some_and(|(a, b)| (a - b).abs() < 1.0), "zoomed out, the bars stay", (y0, y1))?;
    // (Dragged up or down: the rows scroll, if there are more than fit.)
    let rows = one("#timeline-scroll").ok_or("no rows")?;
    let max = rows.scroll_height() - rows.client_height();
    let top0 = rows.scroll_top();
    pointer(&chart, "pointerdown", mid, y);
    pointer(&chart, "pointermove", mid, y + 40.0);
    settle().await;
    pointer(&chart, "pointerup", mid, y + 40.0);
    settle().await;
    check(max <= 0 || rows.scroll_top() != top0, "a drag scrolls the rows", (max, top0, rows.scroll_top()))?;
    // (A click on the overview, wherever it lands: the window's middle there,
    // zoomed in so that it can move.)
    wheel(&chart, mid, y, -1600.0);
    settle().await;
    let over = one(".overview").ok_or("no overview")?;
    let o = over.get_bounding_client_rect();
    let (cx, cy) = (o.left() + o.width() * 0.75, o.top() + o.height() / 2.0);
    // (On what a click there would land on.)
    let hit = doc().element_from_point(cx as f32, cy as f32).unwrap_or(over.clone());
    mouse(&hit, "click", cx, cy, false);
    settle().await;
    let middle = one(".overview .win").map(|w| { let r = w.get_bounding_client_rect(); (r.left() + r.width() / 2.0 - o.left()) / o.width() });
    check(middle.is_some_and(|m| (m - 0.75).abs() < 0.05 || m > 0.75), "a click on the overview moves the window there", middle)?;
    Ok(())
}

/// The editor: a call's completions, a variable's.
async fn editor() -> Result<(), String> {
    // (Typing a call: what fits, offered; Escape, nothing changed.)
    let add = all(".program textarea").into_iter().find(|t| t.get_attribute("placeholder").is_some_and(|p| p.starts_with("add a call")))
        .ok_or("no line to add")?;
    {
        let a = add.dyn_ref::<HtmlTextAreaElement>().ok_or("not a textarea")?;
        let _ = a.focus();
        a.set_value("open");
        let _ = a.set_selection_range(4, 4);
        input(&add);
    }
    settle().await;
    let offered: Vec<String> = all("ul.complete li .mono").iter().map(text).collect();
    // (And a variable of the lines before, as an argument.)
    {
        let a = add.dyn_ref::<HtmlTextAreaElement>().ok_or("not a textarea")?;
        a.set_value("read(f");
        let _ = a.set_selection_range(6, 6);
        input(&add);
    }
    settle().await;
    let names: Vec<String> = all("ul.complete li .mono").iter().map(text).collect();
    // (Seen: what is at the list's middle is the list, not what clips or covers it.)
    let shown = one("ul.complete").map(|u| {
        let r = u.get_bounding_client_rect();
        let top = doc().element_from_point(((r.left() + r.right()) / 2.0) as f32, ((r.top() + r.bottom()) / 2.0) as f32);
        (r.width(), r.height(), top.and_then(|e| e.closest("ul.complete").ok().flatten()).is_some())
    });
    key(&add, "Escape");
    settle().await;
    check(offered.iter().any(|o| o == "openat") && names.iter().any(|o| o == "fd") && shown.is_some_and(|(w, h, seen)| w > 0.0 && h > 0.0 && seen),
          "completions", (&offered, &names, shown))?;
    Ok(())
}

/// Memory (what the read changed) and Tasks (open files; a task_struct opened).
async fn panels(r: &mut Vec<String>) -> Result<(), String> {
    // (Memory: what the read changed, its file's position; Tasks: a task_struct opened.)
    tab("Memory").await;
    let objects: Vec<String> = all(".obj .title").iter().map(text).collect();
    check(named(".obj td", "f_pos").is_some(), "objects", &objects)?;
    r.push(format!("objects {objects:?}"));
    tab("Tasks").await;
    // (The running one, open: its open files, the file the program read
    // among them.)
    let files: Vec<String> = all(".files td.file").iter().map(text).collect();
    check(files.iter().any(|f| f == "/note.txt"), "the running task's open files", &files)?;
    // (An fd clicked: its struct file, its position among its fields.)
    click(&all(".files tr.opens").into_iter().find(|r| text(r).contains("/note.txt")).ok_or("no /note.txt row")?);
    settle().await;
    check(named(".pane td", "f_pos").is_some(), "an open file's struct file", all(".pane td.fname").iter().map(text).collect::<Vec<_>>())?;
    // (Another task selected, then its task_struct.)
    let rows = all(".pane tr").len();
    click(&all(".tasks .task").into_iter().last().ok_or("no task")?);
    settle().await;
    click(&one(".detail .toggle").ok_or("no task_struct toggle")?);
    settle().await;
    let opened = all(".pane tr").len();
    check(opened > rows + 5 && named(".pane td", "comm").is_some(), "a task_struct opened", (rows, opened))?;
    tab("Stack").await;
    Ok(())
}

/// The program changed (the URL says so), back, a try and back, the next
/// lesson and back.
async fn program() -> Result<(), String> {
    // (A line changed: the program made again; 4 bytes read.)
    write(all("ol.lines .cell textarea").get(2).ok_or("no third line")?, "read(fd, buf, 4)");
    let c = || cells().get(2).cloned().unwrap_or_default();
    let made = || format!("{:?}", cells());
    wait(|| c() == ("read(fd, buf, 4)".into(), "= 4".into()), 300, "a line changed", made).await?;
    let loc = web_sys::window().unwrap().location();
    let url = format!("{}{}", loc.pathname().unwrap_or_default(), loc.hash().unwrap_or_default());
    check(url.contains("read.html#p="), "the URL: the lesson's page, the program changed", &url)?;
    // (Back to the lesson's program; a try: its program; and back.)
    click(&one(".ptitle a").ok_or("no reset")?);
    wait(|| c().1 == "= 18", 300, "back to the lesson's program", made).await?;
    click(&named(".explain a", "read twice").ok_or("no try")?);
    wait(|| cells().len() == 5 && done(5), 300, "the try", made).await?;
    let tried: Vec<String> = cells().into_iter().map(|c| c.1).collect();
    check(tried[3] == "= 0", "the try: its second read", &tried)?;
    click(&one(".ptitle a").ok_or("no reset")?);
    wait(|| cells().len() == 3 && done(3), 300, "back to the lesson's program", made).await?;
    // (The next lesson.)
    let title = one("h1").map(|h| text(&h)).unwrap_or_default();
    click(&one(".turn .next").ok_or("no next lesson")?);
    wait(|| one("h1").map(|h| text(&h)) != Some(title.clone()), 100, "the next lesson", || title.clone()).await?;
    // (Its page's URL.)
    let path = web_sys::window().unwrap().location().pathname().unwrap_or_default();
    check(path.ends_with(".html") && !path.ends_with("read.html"), "the next lesson's URL", &path)?;
    // (And back: the link says the lesson's title.)
    let back = all(".turn a").into_iter().next().ok_or("no lesson end")?;
    if !text(&back).ends_with(&title) {
        return Err(format!("the lesson's end says {:?}, not the lesson before", text(&back)));
    }
    click(&back);
    wait(|| one("h1").map(|h| text(&h)) == Some(title.clone()), 100, "the lesson before", || title.clone()).await?;
    Ok(())
}

/// The page opened with a program changed (the URL's p=): its lines made,
/// the text's links still links, and the lesson's own program a click away.
pub(super) async fn url() -> Check {
    wait(|| done(2), 300, "the program made", || format!("{:?}", cells())).await?;
    let c = cells();
    check(c.len() == 2 && c[1].1 == "= 9" && one(".ptitle a").is_some(), "not back", &c)?;
    // (Its text's links into the recording: still links, where the recording has them.)
    wait(|| named(".explain a", "ksys_write").is_some(), 100, "a link, the program changed", || all(".explain a").iter().map(text).collect::<Vec<_>>().join(", ")).await?;
    // (The header's link to the source it was built from: the page's head says which.)
    let source = one("a.build").and_then(|a| a.get_attribute("href")).unwrap_or_default();
    check(source.contains("/tree/"), "the source link", &source)?;
    Ok(format!("{c:?}"))
}

/// The pipe example (two processes: the program, and a child sharing its
/// memory, with fds of its own): the child's read blocked, finished as the
/// program's write woke it; what each call changed; both tasks in the strip.
pub(super) async fn tasks() -> Check {
    wait(|| done(7) && drew("data-test-tasks").is_some(), 300, "the calls made", || format!("{:?}", cells())).await?;
    let c = cells();
    let changed: Vec<String> = all(".cell .more").iter().map(text).collect();
    // (The child's line selected: its task in the strip as the program's other.)
    let _ = all("ol.lines .cell textarea").get(5).ok_or("no sixth line")?.dyn_ref::<HtmlElement>().map(|t| t.focus());
    settle().await;
    click(&all(".charthead a").into_iter().find(|a| text(a).starts_with("zoom to")).ok_or("no zoom to")?);
    settle().await;
    let mut strip: Vec<String> = drew("data-test-tasks").unwrap_or_default().lines().filter(|n| n.starts_with("prog(")).map(String::from).collect();
    strip.sort();
    strip.dedup();
    check(c[5].1 == "= 6 (during write)" && c[6].1 == "= 6" && changed.iter().any(|x| x == "fds = {3, 4}") && strip.len() >= 2,
          "the pipe", (&c, &changed, &strip))?;
    // (The write's: the program's open files, the pipe's two ends.)
    let _ = all("ol.lines .cell textarea").get(6).ok_or("no seventh line")?.dyn_ref::<HtmlElement>().map(|t| t.focus());
    settle().await;
    tab("Tasks").await;
    let files: Vec<String> = all(".files td.file").iter().map(text).collect();
    check(files.iter().filter(|f| f.starts_with("pipefs:[")).count() == 2, "the pipe's ends", &files)?;
    tab("Stack").await;
    // (The idle task, between the calls: a click on its bar, the cursor in
    // its run, as in any task's.)
    let canvas = one("#timeline-canvas").ok_or("no timeline canvas")?;
    let (x, y) = bar(&canvas, "do_idle").ok_or("no do_idle bar")?;
    let b = canvas.get_bounding_client_rect();
    mouse(&one(".chart").ok_or("no chart")?, "click", b.left() + x, b.top() + y, false);
    settle().await;
    let title = one(".charthead .dim").map(|t| text(&t)).unwrap_or_default();
    check(title.contains("swapper"), "a click on the idle task's bar: the cursor in its run", (&title, place()))?;
    Ok(format!("{changed:?} {strip:?}; {title}"))
}

/// The read example on a phone's screen (390x844): the lesson, then the
/// chart, the source, the panels, one under another, the page never wider
/// than the screen; ▶ steps.
pub(super) async fn responsive() -> Check {
    wait(|| done(3) && one("#timeline-canvas").and_then(|c| bar(&c, "vfs_read")).is_some(), 300, "the calls made", || format!("{:?}", cells())).await?;
    let top = |s: &str| one(s).map_or(f64::NAN, |e| e.get_bounding_client_rect().top());
    let order = top(".lesson") < top(".chart") && top(".chart") < top(".source") && top(".source") < top(".panels");
    let wide = doc().document_element().map_or(0, |e| e.scroll_width());
    let screen = web_sys::window().unwrap().inner_width().ok().and_then(|w| w.as_f64()).unwrap_or(0.0) as i32;
    check(order && wide <= screen, "the phone", (order, wide, screen))?;
    // (The source, wider than the screen: scrolled sideways, its lines whole.)
    wait(|| one(".source .code .ln").is_some(), 100, "the source drawn", || String::new()).await?;
    let src = one(".source .scroll").ok_or("no source")?;
    let (sw, cw) = (src.scroll_width(), src.client_width());
    check(sw > cw, "the source scrolls sideways", (sw, cw))?;
    let at = place();
    press("↑ In");
    settle().await;
    Ok(format!("{at} → {}", place()))
}

/// Attribute `attr` of the chart's canvas: its width, or what it drew
/// (data-test-NAME: see chart.rs's draw_chart).
fn drew(attr: &str) -> Option<String> {
    one("#timeline-canvas")?.get_attribute(attr)
}

/// Where the chart drew a bar of function `name` (its middle, on the
/// canvas): the first it lists.
fn bar(canvas: &web_sys::Element, name: &str) -> Option<(f64, f64)> {
    bars(canvas).into_iter().find(|b| b.0 == name).map(|b| (b.2, b.3))
}

/// The bars the chart drew: each call's function, its first record, the
/// middle of what is seen of it (x, y), its left end and its width.
fn bars(canvas: &web_sys::Element) -> Vec<(String, String, f64, f64, f64, f64)> {
    canvas.get_attribute("data-test-bars").unwrap_or_default().lines().filter_map(|l| {
        let mut f = l.split(' ');
        let (name, call, at, left, w) = (f.next()?, f.next()?, f.next()?, f.next()?, f.next()?);
        let (x, y) = at.split_once(',')?;
        Some((name.into(), call.into(), x.parse().ok()?, y.parse().ok()?, left.parse().ok()?, w.parse().ok()?))
    }).collect()
}

/// The durable lesson (durable.html): the Disk tab, the disk's history;
/// the cursor in the first call, what comes after it gray; in the fsync,
/// its writes, jbd2's (the journal's) among them, bytes changed; a row's
/// moments, handed over and done.
pub(super) async fn disk() -> Check {
    wait(|| done(3), 600, "the calls made", || format!("{:?}", cells())).await?;
    let line = |k: usize| all("ol.lines .cell textarea").get(k).and_then(|t| t.dyn_ref::<HtmlElement>().map(|t| t.focus()));
    line(0);
    settle().await;
    tab("Disk").await;
    wait(|| !all(".disk .xfer").is_empty(), 50, "the disk's transfers", || one(".pane").map(|p| text(&p)).unwrap_or_default()).await?;
    let later = all(".disk .xfer.later").len();
    check(later > 0, "the cursor in the first call: the transfers after it gray", later)?;
    line(2);
    settle().await;
    let rows: Vec<String> = all(".disk .xfer").iter().map(text).collect();
    check(rows.iter().any(|r| r.starts_with("write") && r.contains("changed")) && rows.iter().any(|r| r.contains("by jbd2")),
          "the fsync's writes, bytes changed, jbd2's among them", &rows)?;
    // (A recorded row, clicked anywhere: the cursor where the driver handed
    // it over; its "done": where the disk's interrupt took it as done.)
    let row = all(".disk .xfer").into_iter().find(|r| r.query_selector("a.dim").ok().flatten().is_some()).ok_or("no recorded transfer")?;
    click(&row.query_selector(".bytes").ok().flatten().ok_or("no bytes")?);
    settle().await;
    let handed = place();
    tab("Disk").await;
    click(&one(".disk .xfer a.dim").ok_or("no done")?);
    settle().await;
    let done = place();
    check(handed.starts_with("wasm_disk_queue_rq") && done.starts_with("wasm_disk_interrupt"), "a transfer's moments: handed over, done", (&handed, &done))?;
    Ok(format!("{} transfers, {later} after the first call; {}; {handed}, {done}", rows.len(), rows.first().cloned().unwrap_or_default()))
}
