//! A lesson's paragraphs: text with `code` and [links](call:function) into
//! the recording (see web/examples.js for what a link names).

use dioxus::prelude::*;

/// A piece of a paragraph.
#[derive(Clone, PartialEq, Debug)]
pub enum Piece {
    Text(String),
    Code(String),
    /// [label](target): `call`, the call it names (from 1), if a number.
    Link { label: String, target: String, call: Option<usize> },
}

/// Paragraph `p`'s pieces.
pub fn pieces(p: &str) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut rest = p;
    while !rest.is_empty() {
        let next = rest.find(['`', '[']).unwrap_or(rest.len());
        if next > 0 {
            out.push(Piece::Text(rest[..next].into()));
            rest = &rest[next..];
            continue;
        }
        if let Some(r) = rest.strip_prefix('`') && let Some(end) = r.find('`') {
            out.push(Piece::Code(r[..end].into()));
            rest = &r[end + 1..];
            continue;
        }
        if let Some(r) = rest.strip_prefix('[') && let Some(close) = r.find("](") && let Some(end) = r[close..].find(')') {
            let (label, target) = (&r[..close], &r[close + 2..close + end]);
            let call = target.split(':').next().and_then(|n| n.parse().ok());
            out.push(Piece::Link { label: label.into(), target: target.into(), call });
            rest = &r[close + end + 1..];
            continue;
        }
        out.push(Piece::Text(rest[..1].into()));
        rest = &rest[1..];
    }
    out
}

/// The calls paragraph `p` links to, in order.
pub fn calls(p: &str) -> Vec<usize> {
    pieces(p).into_iter().filter_map(|x| match x {
        Piece::Link { call, .. } => call,
        _ => None,
    }).collect()
}

#[component]
pub fn Paragraph(text: String) -> Element {
    rsx! {
        p { class: "para",
            for x in pieces(&text) {
                match x {
                    Piece::Text(t) => rsx! { "{t}" },
                    Piece::Code(c) => rsx! { code { "{c}" } },
                    Piece::Link { label, target, .. } => rsx! { a { class: "link", title: "{target}", "{label}" } },
                }
            }
        }
    }
}
