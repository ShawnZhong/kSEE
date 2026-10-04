//! Page behavior exercised through its rendered DOM.

use wasm_bindgen::JsCast;
use web_sys::{HtmlElement, HtmlInputElement, HtmlTextAreaElement};

use super::Check;
use super::dom::*;

/// The read example (#ex=read): its calls made, its subject (the read)
/// selected; its links followed; stepping into, over, out, and by key; the
/// chart (a click, find); Objects and Tasks; a line changed, a
/// try and back; the next lesson.
pub(super) async fn lesson() -> Check {
    let mut r = Vec::new();
    wait(
        || {
            done(3)
                && one("#timeline-canvas")
                    .and_then(|c| c.get_attribute("width"))
                    .and_then(|w| w.parse::<u32>().ok())
                    .is_some_and(|w| w > 0)
        },
        900,
        "the calls made",
        || {
            let status = one(".pad.dim").map(|e| text(&e)).unwrap_or_default();
            format!("{:?}; {status}", cells())
        },
    )
    .await?;
    let selected = one(".cell.on textarea")
        .and_then(|t| t.dyn_into::<HtmlTextAreaElement>().ok())
        .map(|t| t.value())
        .unwrap_or_default();
    check(
        selected.starts_with("read("),
        "the subject call selected",
        &selected,
    )?;
    // (The source's file is fetched: drawn a moment later.)
    let drawn = || {
        [
            one("#timeline-canvas")
                .and_then(|c| c.get_attribute("width"))
                .and_then(|w| w.parse::<usize>().ok())
                .unwrap_or(0),
            one("#timeline-canvas")
                .and_then(|c| c.get_attribute("data-test-tasks"))
                .is_some() as usize,
            all(".source .ln").len(),
            all("tr.frame").len(),
        ]
    };
    wait(
        || drawn().iter().all(|&n| n > 0),
        100,
        "drawn (bars, tasks, source, frames)",
        || format!("{:?}", drawn()),
    )
    .await?;
    let drawn = drawn();
    r.push(format!("drawn {drawn:?}"));
    // (The links: each in its function, the source at its line.)
    for name in ["ksys_read", "vfs_read", "filemap_read"] {
        click(&named(".explain a", name).ok_or(fail("no link", name))?);
        let ok = until(
            || place().starts_with(name) && one(".source .ln.now").is_some(),
            50,
        )
        .await;
        check(ok, &format!("the link to {name}"), place())?;
        r.push(place());
    }
    // (The global key listener: reset to a known cursor, then move it.)
    click(&named(".explain a", "vfs_read").ok_or("no link to vfs_read")?);
    sleep(300).await;
    let cursor = || {
        one("#timeline-canvas")
            .and_then(|e| e.get_attribute("data-test-now"))
            .unwrap_or_default()
    };
    let before = cursor();
    global_key("ArrowRight");
    wait(
        || cursor() != before,
        20,
        "the global → key",
        || format!("{} at {}", cursor(), place()),
    )
    .await?;
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
    check(
        into != at && over.starts_with("vfs_read"),
        "stepping",
        (&at, &into, &over),
    )?;
    // (Hover and click resolve the same canvas bar and moment.)
    let canvas = one("#timeline-canvas").ok_or("no timeline canvas")?;
    let point = canvas
        .get_attribute("data-test-vfs-read")
        .ok_or("no vfs_read canvas bar")?;
    let (x, y) = point.split_once(',').ok_or("bad vfs_read canvas point")?;
    let b = canvas.get_bounding_client_rect();
    let chart = one(".chart").ok_or("no chart")?;
    let (x, y) = (
        b.left() + x.parse::<f64>().map_err(|_| "bad canvas x")?,
        b.top() + y.parse::<f64>().map_err(|_| "bad canvas y")?,
    );
    pointer(&chart, "pointermove", x, y);
    sleep(200).await;
    let hovered = place();
    mouse(&chart, "click", x, y, false);
    sleep(300).await;
    let chosen = one("tr.frame.on a").map(|a| text(&a)).unwrap_or_default();
    check(
        chosen == "vfs_read" && hovered == place(),
        "hover and click on a bar",
        (&chosen, &hovered, place()),
    )?;
    // (Find: its bars lit; Enter, the cursor in it.)
    let find = one(".find").ok_or("no find box")?;
    find.dyn_ref::<HtmlInputElement>()
        .ok_or("find, not an input")?
        .set_value("copy_page_to_iter");
    input(&find);
    sleep(200).await;
    let found = canvas
        .get_attribute("data-test-found")
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    key(&find, "Enter");
    sleep(300).await;
    check(
        found > 0 && named("tr.frame a", "copy_page_to_iter").is_some(),
        "find",
        (found, place()),
    )?;
    // (A drag pans, zoomed in: the bars follow the pointer, and the window
    // stays where it let go.)
    let mid = (b.left() + b.right()) / 2.0;
    wheel(&chart, mid, y, -800.0);
    sleep(300).await;
    let bar_x = || canvas.get_attribute("data-test-vfs-read").and_then(|p| p.split_once(',')?.0.parse::<f64>().ok()).unwrap_or(f64::NAN);
    let win = || one(".overview .win").and_then(|w| w.get_attribute("style")).unwrap_or_default();
    let (x0, w0) = (bar_x(), win());
    pointer(&chart, "pointerdown", mid, y);
    pointer(&chart, "pointermove", mid - 80.0, y);
    sleep(200).await;
    let during = bar_x();
    pointer(&chart, "pointerup", mid - 80.0, y);
    sleep(300).await;
    check((x0 - 80.0 - during).abs() < 1.0 && win() != w0, "a drag pans", (x0, during, w0, win()))?;
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
    check(
        opened > rows + 5 && named(".pane td", "comm").is_some(),
        "a task_struct opened",
        (rows, opened),
    )?;
    tab("Stack").await;
    // (A line changed: the program made again; 4 bytes read.)
    write(
        all("ol.steps .cell textarea")
            .get(2)
            .ok_or("no third line")?,
        "read(fd, buf, 4)",
    );
    let c = || cells().get(2).cloned().unwrap_or_default();
    wait(
        || c() == ("read(fd, buf, 4)".into(), "= 4".into()),
        300,
        "a line changed",
        || format!("{:?}", cells()),
    )
    .await?;
    let hash = web_sys::window()
        .unwrap()
        .location()
        .hash()
        .unwrap_or_default();
    check(hash.contains("&p="), "the URL: the program changed", &hash)?;
    // (Back to the lesson's program; a try: its program; and back.)
    click(&one(".ptitle a").ok_or("no reset")?);
    wait(
        || c().1 == "= 18",
        300,
        "back to the lesson's program",
        || format!("{:?}", cells()),
    )
    .await?;
    click(&named(".explain a", "read twice").ok_or("no try")?);
    wait(
        || cells().len() == 5 && done(5),
        300,
        "the try",
        || format!("{:?}", cells()),
    )
    .await?;
    let tried: Vec<String> = cells().into_iter().map(|c| c.1).collect();
    check(tried[3] == "= 0", "the try: its second read", &tried)?;
    click(&one(".ptitle a").ok_or("no reset")?);
    wait(
        || cells().len() == 3 && done(3),
        300,
        "back to the lesson's program",
        || format!("{:?}", cells()),
    )
    .await?;
    // (The next lesson.)
    let title = one("h1").map(|h| text(&h)).unwrap_or_default();
    click(&one(".topbar button[title='the next lesson']").ok_or("no next lesson")?);
    wait(
        || one("h1").map(|h| text(&h)) != Some(title.clone()),
        100,
        "the next lesson",
        || title.clone(),
    )
    .await?;
    Ok(r.join("; "))
}

/// The page opened with a program changed (the URL's p=): its lines made,
/// said to be a try, and the lesson's own program a click away.
pub(super) async fn history() -> Check {
    wait(
        || done(2),
        300,
        "the program made",
        || format!("{:?}", cells()),
    )
    .await?;
    let note = one(".explain.dim").map(|n| text(&n)).unwrap_or_default();
    let c = cells();
    check(
        c.len() == 2
            && c[1].1 == "= 9"
            && note.contains("not the lesson's own program")
            && one(".ptitle a").is_some(),
        "not back",
        (&c, &note),
    )?;
    Ok(format!("{c:?}"))
}

/// The pipe example (two processes: the program, and a child sharing its
/// memory, with fds of its own): the child's read blocked, finished as the
/// program's write woke it; what each call changed; both tasks in the strip.
pub(super) async fn tasks() -> Check {
    wait(
        || {
            done(7)
                && one("#timeline-canvas")
                    .and_then(|c| c.get_attribute("data-test-tasks"))
                    .is_some()
        },
        300,
        "the calls made",
        || format!("{:?}", cells()),
    )
    .await?;
    let c = cells();
    let changed: Vec<String> = all(".cell .more").iter().map(text).collect();
    // (The child's line selected: its task in the strip as the program's other.)
    let _ = all("ol.steps .cell textarea")
        .get(5)
        .ok_or("no sixth line")?
        .dyn_ref::<HtmlElement>()
        .map(|t| t.focus());
    sleep(500).await;
    let mut strip: Vec<String> = one("#timeline-canvas")
        .and_then(|c| c.get_attribute("data-test-tasks"))
        .unwrap_or_default()
        .lines()
        .filter(|n| n.starts_with("prog("))
        .map(String::from)
        .collect();
    strip.sort();
    strip.dedup();
    check(
        c[5].1 == "= 6 (during line 7)"
            && c[6].1 == "= 6"
            && changed.iter().any(|x| x == "fds = {3, 4}")
            && strip.len() >= 2,
        "the pipe",
        (&c, &changed, &strip),
    )?;
    Ok(format!("{changed:?} {strip:?}"))
}

/// The read example on a phone's screen (390x844): the lesson, then the
/// chart, the source, the panels, one under another, the page never wider
/// than the screen; ▶ steps.
pub(super) async fn responsive() -> Check {
    wait(
        || {
            done(3)
                && one("#timeline-canvas")
                    .and_then(|c| c.get_attribute("data-test-vfs-read"))
                    .is_some()
        },
        300,
        "the calls made",
        || format!("{:?}", cells()),
    )
    .await?;
    let top = |s: &str| {
        one(s)
            .map(|e| e.get_bounding_client_rect().top())
            .unwrap_or(f64::NAN)
    };
    let order = top(".lesson") < top(".chart")
        && top(".chart") < top(".source")
        && top(".source") < top(".panels");
    let wide = doc()
        .document_element()
        .map(|e| e.scroll_width())
        .unwrap_or(0);
    let screen = web_sys::window()
        .unwrap()
        .inner_width()
        .ok()
        .and_then(|w| w.as_f64())
        .unwrap_or(0.0) as i32;
    check(order && wide <= screen, "the phone", (order, wide, screen))?;
    let at = place();
    press("▶", false);
    sleep(300).await;
    Ok(format!("{at} → {}", place()))
}
