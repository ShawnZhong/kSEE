//! The lessons (user/lessons/NAME.json), for the site: lessons.json, the
//! page's; and the page for each, NAME.html (and index.html, the first's
//! place): web/index.html with the lesson's title and description, and its
//! text as plain HTML (#static), which a search engine or a link preview
//! reads without running the page, and which the page, once it runs,
//! replaces.

use std::path::Path;

use ksee_core::text::{pieces, Piece};
use serde_json::{json, Value};

use crate::build::SITE;

/// The course: its chapters, each its lessons (user/lessons/NAME.json)
/// in order, from the simplest.
const COURSE: &[(&str, &[&str])] = &[
    ("Start here", &["umask", "umask-inside"]),
    ("A process", &["getpid", "prctl", "exec", "thread"]),
    ("Files, in memory", &["hello", "open-close", "dup", "read", "write", "open-fails", "create", "dir"]),
    ("Files on a disk (ext4)", &["ext4-read", "ext4-create", "ext4-write", "durable", "ext4-dir", "link", "remove"]),
    ("Waiting and waking", &["sleep", "futex", "join", "pipe", "kill"]),
    ("Your own", &["yours"]),
];

/// lessons.json and the pages, from user/lessons/*.json (a file not in
/// the course is left out) and web/index.html.
pub fn write() {
    eprintln!("+ (lessons) user/lessons/*.json, web/index.html -> {SITE}/lessons.json, {SITE}/NAME.html");
    let mut all: Vec<Value> = Vec::new();
    for &(chapter, names) in COURSE {
        for &name in names {
            let path = format!("user/lessons/{name}.json");
            let mut lesson: Value = crate::or_fail(serde_json::from_slice(&crate::read(Path::new(&path))), &path);
            lesson["name"] = json!(name);
            lesson["chapter"] = json!(chapter);
            all.push(lesson);
        }
    }
    let page = String::from_utf8_lossy(&crate::read(Path::new("web/index.html"))).into_owned();
    let out = |name: &str, text: String| {
        let out = format!("{SITE}/{name}");
        // (Not through a link an older build left: to web/'s file.)
        let _ = std::fs::remove_file(&out);
        crate::or_fail(std::fs::write(&out, text), &out);
    };
    out("lessons.json", serde_json::to_string(&all).unwrap_or_default());
    for lesson in &all {
        out(&format!("{}.html", s(lesson, "name")), fill(&page, &format!("{} — kSEE", s(lesson, "title")), s(lesson, "question"), &as_text(lesson)));
    }
    out("index.html", fill(&page, TITLE, DESCRIPTION, &contents(&all)));
    // (For search engines: every page, at the address the site is published
    // at, if it is known. Given to them by hand (Google's Search Console):
    // a robots.txt naming it would have to be at the host's root.)
    if let Some(base) = crate::build::published() {
        let urls: String = std::iter::once(String::new()).chain(all.iter().map(|lesson| format!("{}.html", s(lesson, "name"))))
            .map(|page| format!("  <url><loc>{base}{page}</loc></url>\n")).collect();
        out("sitemap.xml", format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n{urls}</urlset>\n"));
    }
}

const TITLE: &str = "kSEE, the kernel Syscall Execution Explorer: step through a real Linux kernel in your browser";
const DESCRIPTION: &str = "kSEE runs a real Linux kernel in your browser and records everything a system call does: \
                           step through its source line by line, with the call stack, its arguments and locals, and every write to memory.";

/// A lesson's string field.
fn s<'a>(lesson: &'a Value, key: &str) -> &'a str {
    lesson[key].as_str().unwrap_or_default()
}

fn escape(t: &str) -> String {
    t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The page, its title and description `title` and `description` (the
/// page's own, and a link preview's), and `body` its static text.
fn fill(page: &str, title: &str, description: &str, body: &str) -> String {
    let (title, description) = (escape(title), escape(description));
    page.lines().map(|l| {
        let t = l.trim_start();
        if t.starts_with("<title>") {
            format!("<title>{title}</title>")
        } else if t.starts_with("<meta name=\"description\"") || t.starts_with("<meta property=\"og:description\"") {
            l.split("content=").next().map(|a| format!("{a}content=\"{description}\">")).unwrap_or_default()
        } else if t.starts_with("<meta property=\"og:title\"") {
            format!("<meta property=\"og:title\" content=\"{title}\">")
        } else if t == "<div id=\"root\"></div>" {
            format!("{l}\n<article id=\"static\">\n{body}</article>")
        } else {
            l.to_string()
        }
    }).collect::<Vec<_>>().join("\n") + "\n"
}

/// A lesson's paragraph as HTML (ksee_core::text's pieces): `code`, and a
/// link's label (what it points to is in the recording: the page has it).
fn paragraph(t: &str) -> String {
    pieces(t).into_iter().map(|p| match p {
        Piece::Text(t) => escape(&t),
        Piece::Code(c) => format!("<code>{}</code>", escape(&c)),
        Piece::Link { label, .. } => format!("<em>{}</em>", paragraph(&label)),
    }).collect()
}

/// Lesson `lesson`, as text: its chapter, title, question and text, its program.
fn as_text(lesson: &Value) -> String {
    let mut h = format!("<p><a href=\"./\">kSEE</a> · {}</p>\n<h1>{}</h1>\n<p>{}</p>\n",
                        escape(s(lesson, "chapter")), escape(s(lesson, "title")), escape(s(lesson, "question")));
    for p in lesson["text"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        h += &format!("<p>{}</p>\n", paragraph(p));
    }
    h += "<h2>The program</h2>\n<ol>\n";
    for step in lesson["program"].as_array().into_iter().flatten() {
        // (A step: its line, or a task's: {task, text}.)
        let (task, text) = match step.as_str() {
            Some(t) => (None, t),
            None => (step["task"].as_str(), s(step, "text")),
        };
        let by = task.map_or(String::new(), |t| format!(" (by {})", escape(t)));
        h += &format!("<li><code>{}</code>{by}</li>\n", escape(text));
    }
    h + "</ol>\n"
}

/// The lessons, by chapter: the site's own page's text.
fn contents(all: &[Value]) -> String {
    let mut h = format!("<h1>kSEE</h1>\n<p>{}</p>\n", escape(DESCRIPTION));
    let mut chapter = "";
    for lesson in all {
        if s(lesson, "chapter") != chapter {
            chapter = s(lesson, "chapter");
            h += &format!("<h2>{}</h2>\n", escape(chapter));
        }
        h += &format!("<p><a href=\"{}.html\">{}</a>: {}</p>\n", s(lesson, "name"), escape(s(lesson, "title")), escape(s(lesson, "question")));
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_paragraph_s_code_and_links() {
        assert_eq!(paragraph("a `x < y` and [`ksys_read`](1:ksys_read), [twice](try:1) & more"),
                   "a <code>x &lt; y</code> and <em><code>ksys_read</code></em>, <em>twice</em> &amp; more");
    }

    #[test]
    fn a_lesson_s_page() {
        let page = "<head>\n<title>x</title>\n<meta name=\"description\" content=\"x\">\n<meta property=\"og:title\" content=\"x\">\n</head>\n<body>\n<div id=\"root\"></div>\n</body>\n";
        let lesson = json!({ "name": "r", "chapter": "G", "title": "T", "question": "Q?", "text": ["P `c`."],
                         "program": ["read(fd)", { "task": "child", "text": "exit(0)" }] });
        let h = fill(page, "T — kSEE", "Q?", &as_text(&lesson));
        for want in ["<title>T — kSEE</title>", "content=\"Q?\">", "og:title\" content=\"T — kSEE\"", "<div id=\"root\"></div>\n<article id=\"static\">",
                     "<h1>T</h1>", "<p>P <code>c</code>.</p>", "<code>read(fd)</code>", "(by child)"] {
            assert!(h.contains(want), "{want} in {h}");
        }
    }
}
