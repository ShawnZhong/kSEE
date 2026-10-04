//! The lessons (user/examples/NAME.json), for the site: examples.json, the
//! page's; and each lesson as a page of its own (lessons/NAME.html, and
//! their contents, lessons/index.html), plain HTML that a search engine
//! or a link preview reads without running the page.

use std::path::Path;

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

/// examples.json and the lessons' pages, from user/examples/*.json (a file
/// not in the course is left out).
pub fn write() {
    eprintln!("+ (lessons) user/examples/*.json -> {SITE}/examples.json, {SITE}/lessons/");
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
    let out = format!("{SITE}/examples.json");
    crate::or_fail(std::fs::write(&out, serde_json::to_string(&all).unwrap_or_default()), &out);
    let dir = format!("{SITE}/lessons");
    crate::or_fail(std::fs::create_dir_all(&dir), &dir);
    for (k, ex) in all.iter().enumerate() {
        let out = format!("{dir}/{}.html", s(ex, "name"));
        crate::or_fail(std::fs::write(&out, page(ex, k.checked_sub(1).and_then(|j| all.get(j)), all.get(k + 1))), &out);
    }
    let out = format!("{dir}/index.html");
    crate::or_fail(std::fs::write(&out, contents(&all)), &out);
}

/// A lesson's string field.
fn s<'a>(ex: &'a Value, key: &str) -> &'a str {
    ex[key].as_str().unwrap_or_default()
}

fn escape(t: &str) -> String {
    t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A lesson's paragraph as HTML: `code`, and a [link](target) its label
/// (what it points to is in the recording: the page has it).
fn paragraph(t: &str) -> String {
    let mut out = String::new();
    let mut rest = t;
    while let Some(k) = rest.find(['`', '[']) {
        out += &escape(&rest[..k]);
        let tail = &rest[k..];
        let (html, used) = if let Some(code) = tail.strip_prefix('`').and_then(|c| c.split_once('`')) {
            (format!("<code>{}</code>", escape(code.0)), code.0.len() + 2)
        } else if let Some((label, after)) = tail[1..].split_once("](").and_then(|(l, a)| Some((l, a.split_once(')')?.1))) {
            (format!("<em>{}</em>", paragraph(label)), tail.len() - after.len())
        } else {
            (escape(&tail[..1]), 1)
        };
        out += &html;
        rest = &tail[used..];
    }
    out + &escape(rest)
}

const STYLE: &str = "body{font:16px/1.55 system-ui,sans-serif;max-width:44rem;margin:2rem auto;padding:0 1rem;color:#1f2328;background:#fbfbfa}\
a{color:#0969da}code{font:14px ui-monospace,Menlo,monospace}.dim{color:#6b7280}ol code{white-space:pre-wrap}\
@media(prefers-color-scheme:dark){body{color:#e6edf3;background:#0d1117}a{color:#58a6ff}.dim{color:#8b949e}}";

fn head(title: &str, description: &str) -> String {
    format!("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
             <title>{t}</title>\n<meta name=\"description\" content=\"{d}\">\n<meta property=\"og:title\" content=\"{t}\">\n\
             <meta property=\"og:description\" content=\"{d}\">\n<style>{STYLE}</style>\n</head>\n<body>\n",
            t = escape(title), d = escape(description))
}

/// Lesson `ex`'s page: its chapter, title, question and text, its program,
/// a link to run it in kSEE, and the lessons before and after.
fn page(ex: &Value, before: Option<&Value>, after: Option<&Value>) -> String {
    let (name, title) = (s(ex, "name"), s(ex, "title"));
    let mut h = head(&format!("{title} — kSEE"), s(ex, "question"));
    h += &format!("<p class=\"dim\"><a href=\"./\">kSEE lessons</a> · {}</p>\n<h1>{}</h1>\n<p class=\"dim\">{}</p>\n",
                  escape(s(ex, "group")), escape(title), escape(s(ex, "question")));
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
        let by = task.map_or(String::new(), |t| format!(" <span class=\"dim\">(by {})</span>", escape(t)));
        h += &format!("<li><code>{}</code>{by}</li>\n", escape(text));
    }
    h += &format!("</ol>\n<p><a href=\"../#ex={name}\"><b>Run it in kSEE ▶</b></a> <span class=\"dim\">(a real Linux kernel, in your browser)</span></p>\n<p>");
    if let Some(b) = before {
        h += &format!("<a href=\"{}.html\">◀ {}</a> ", s(b, "name"), escape(s(b, "title")));
    }
    if let Some(a) = after {
        h += &format!("<a href=\"{}.html\" style=\"float:right\">{} ▶</a>", s(a, "name"), escape(s(a, "title")));
    }
    h + "</p>\n</body>\n</html>\n"
}

/// The lessons' contents, by chapter.
fn contents(all: &[Value]) -> String {
    let mut h = head("kSEE lessons: step through a real Linux kernel",
                     "Lessons on what Linux system calls do, each run on a real Linux kernel in your browser and stepped through line by line.");
    h += "<h1>kSEE lessons</h1>\n<p class=\"dim\">kSEE, the kernel Syscall Execution Explorer: step through a real Linux kernel, in your browser. <a href=\"../\">Open kSEE ▶</a></p>\n";
    let mut group = "";
    for ex in all {
        if s(ex, "group") != group {
            group = s(ex, "group");
            h += &format!("<h2>{}</h2>\n", escape(group));
        }
        h += &format!("<p><a href=\"{}.html\">{}</a><br><span class=\"dim\">{}</span></p>\n", s(ex, "name"), escape(s(ex, "title")), escape(s(ex, "question")));
    }
    h + "</body>\n</html>\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_paragraph_s_code_and_links() {
        assert_eq!(paragraph("a `x < y` and [`ksys_read`](1:ksys_read), [twice](try:1) & more [no"),
                   "a <code>x &lt; y</code> and <em><code>ksys_read</code></em>, <em>twice</em> &amp; more [no");
    }

    #[test]
    fn a_lesson_s_page() {
        let ex = json!({ "name": "r", "group": "G", "title": "T", "question": "Q?", "text": ["P `c`."],
                         "steps": ["read(fd)", { "task": "child", "text": "exit(0)" }] });
        let h = page(&ex, None, None);
        for want in ["<title>T — kSEE</title>", "<h1>T</h1>", "<p>P <code>c</code>.</p>", "<code>read(fd)</code>", "(by child)", "../#ex=r"] {
            assert!(h.contains(want), "{want} in {h}");
        }
    }
}
