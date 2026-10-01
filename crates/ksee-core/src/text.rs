//! A lesson's paragraphs: text with `code` and [links](target) into the
//! recording (see lesson::follow for what a target names).

use alloc::string::String;
use alloc::vec::Vec;

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
        } else if let Some(r) = rest.strip_prefix('`') && let Some(end) = r.find('`') {
            out.push(Piece::Code(r[..end].into()));
            rest = &r[end + 1..];
        } else if let Some(r) = rest.strip_prefix('[') && let Some(close) = r.find("](") && let Some(end) = r[close..].find(')') {
            let (label, target) = (&r[..close], &r[close + 2..close + end]);
            let call = target.split(':').next().and_then(|n| n.parse().ok());
            out.push(Piece::Link { label: label.into(), target: target.into(), call });
            rest = &r[close + end + 1..];
        } else {
            out.push(Piece::Text(rest[..1].into()));
            rest = &rest[1..];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_paragraphs_pieces() {
        let p = pieces("1. [vfs_read](2:vfs_read) reads `buf`; [try](try:1).");
        assert_eq!(p, [
            Piece::Text("1. ".into()),
            Piece::Link { label: "vfs_read".into(), target: "2:vfs_read".into(), call: Some(2) },
            Piece::Text(" reads ".into()),
            Piece::Code("buf".into()),
            Piece::Text("; ".into()),
            Piece::Link { label: "try".into(), target: "try:1".into(), call: None },
            Piece::Text(".".into()),
        ]);
    }
}
