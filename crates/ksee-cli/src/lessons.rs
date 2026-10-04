//! The lessons (user/examples/NAME.json), for the site: examples.json, the
//! page's; and the page for each, NAME.html (and index.html, the first's
//! place): web/index.html with the lesson's title and description, and its
//! text as plain HTML (#static), which a search engine or a link preview
//! reads without running the page, and which the page, once it runs,
//! replaces.

use std::path::Path;

use ksee_core::text::{pieces, Piece};
use serde_json::{json, Value};

use crate::build::SITE;

/// The course: its chapters, each its examples (user/examples/NAME.json)
/// in order, from the simplest.
const EXAMPLES: &[(&str, &[&str])] = &[
    ("Start here", &["umask", "umask-inside"]),
    ("A process", &["getpid", "prctl", "exec", "thread"]),
    ("Files, in memory", &["hello", "open-close", "dup", "read", "write", "open-fails", "create", "dir"]),
    ("Files on a disk (ext4)", &["ext4-read", "ext4-create", "ext4-write", "durable", "ext4-dir", "link", "remove"]),
    ("Waiting and waking", &["sleep", "futex", "join", "pipe", "kill"]),
    ("Your own", &["yours"]),
];

/// examples.json and the pages, from user/examples/*.json (a file not in
/// the course is left out) and web/index.html.
pub fn write() {
    eprintln!("+ (lessons) user/examples/*.json, web/index.html -> {SITE}/examples.json, {SITE}/NAME.html");
    let mut all: Vec<Value> = Vec::new();
    for &(group, names) in EXAMPLES {
        for &name in names {
            let path = format!("user/examples/{name}.json");
            let mut ex: Value = crate::or_fail(serde_json::from_slice(&crate::read(Path::new(&path))), &path);
            ex["name"] = json!(name);
            ex["group"] = json!(group);
            all.push(ex);
        }
    }
    let page = String::from_utf8_lossy(&crate::read(Path::new("web/index.html"))).into_owned();
    let out = |name: &str, text: String| {
        let out = format!("{SITE}/{name}");
        // (Not through a link: the site's files are web/'s, linked.)
        let _ = std::fs::remove_file(&out);
        crate::or_fail(std::fs::write(&out, text), &out);
    };
    out("examples.json", serde_json::to_string(&all).unwrap_or_default());
    for ex in &all {
        out(&format!("{}.html", s(ex, "name")), fill(&page, &format!("{} — kSEE", s(ex, "title")), s(ex, "question"), &lesson(ex)));
    }
    out("index.html", fill(&page, TITLE, DESCRIPTION, &contents(&all)));
}

const TITLE: &str = "kSEE, the kernel Syscall Execution Explorer: step through a real Linux kernel in your browser";
const DESCRIPTION: &str = "kSEE runs a real Linux kernel in your browser and records everything a system call does: \
                           step through its source line by line, with the call stack, its arguments and locals, and every write to memory.";

/// A lesson's string field.
fn s<'a>(ex: &'a Value, key: &str) -> &'a str {
    ex[key].as_str().unwrap_or_default()
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

/// Lesson `ex`, as text: its chapter, title, question and text, its program.
fn lesson(ex: &Value) -> String {
    let mut h = format!("<p><a href=\"./\">kSEE</a> · {}</p>\n<h1>{}</h1>\n<p>{}</p>\n",
                        escape(s(ex, "group")), escape(s(ex, "title")), escape(s(ex, "question")));
    for p in ex["text"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        h += &format!("<p>{}</p>\n", paragraph(p));
    }
    h += "<h2>The program</h2>\n<ol>\n";
    for step in ex["steps"].as_array().into_iter().flatten() {
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
    let mut group = "";
    for ex in all {
        if s(ex, "group") != group {
            group = s(ex, "group");
            h += &format!("<h2>{}</h2>\n", escape(group));
        }
        h += &format!("<p><a href=\"{}.html\">{}</a>: {}</p>\n", s(ex, "name"), escape(s(ex, "title")), escape(s(ex, "question")));
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
        let ex = json!({ "name": "r", "group": "G", "title": "T", "question": "Q?", "text": ["P `c`."],
                         "steps": ["read(fd)", { "task": "child", "text": "exit(0)" }] });
        let h = fill(page, "T — kSEE", "Q?", &lesson(&ex));
        for want in ["<title>T — kSEE</title>", "content=\"Q?\">", "og:title\" content=\"T — kSEE\"", "<div id=\"root\"></div>\n<article id=\"static\">",
                     "<h1>T</h1>", "<p>P <code>c</code>.</p>", "<code>read(fd)</code>", "(by child)"] {
            assert!(h.contains(want), "{want} in {h}");
        }
    }
}
