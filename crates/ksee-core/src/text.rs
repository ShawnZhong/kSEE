//! A lesson's paragraphs: text with `code` and [links](target) into the
//! recording.

use alloc::string::String;
use alloc::vec::Vec;

/// A panel a lesson link opens.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LessonTab {
    Stack,
    Memory,
    Tasks,
    Console,
    Disk,
}

impl LessonTab {
    fn parse(part: &str) -> Option<Self> {
        match part {
            "stack" => Some(Self::Stack),
            "memory" => Some(Self::Memory),
            "tasks" => Some(Self::Tasks),
            "console" => Some(Self::Console),
            "disk" => Some(Self::Disk),
            _ => None,
        }
    }
}

/// What a lesson link names.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LessonTarget {
    /// The lesson's try `index` (from 1), made instead of its program.
    Try { index: usize },
    /// A tab, whatever the recording has.
    Tab(LessonTab),
    /// Where `function` is first called: in the program's `n`th call named
    /// `call` (a syscall's name), if it says, else in its first call that
    /// ran it; then a tab, maybe.
    Recording { call: Option<(String, usize)>, function: String, tab: Option<LessonTab> },
}

impl LessonTarget {
    /// Parse `try:N`, `tab`, or `[call[#n]/]function[/tab]`.
    pub fn parse(target: &str) -> Option<Self> {
        if let Some(n) = target.strip_prefix("try:") {
            return Some(Self::Try { index: positive(n)? });
        }
        let mut parts: Vec<&str> = target.split('/').collect();
        let tab = match parts.last().and_then(|t| LessonTab::parse(t)) {
            Some(t) => (parts.pop(), Some(t)).1,
            None => None,
        };
        let name = |s: &str| (!s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')).then(|| s.into());
        match parts[..] {
            [] => Some(Self::Tab(tab?)),
            [function] => Some(Self::Recording { call: None, function: name(function)?, tab }),
            [call, function] => {
                let (call, n) = call.split_once('#').map_or(Some((call, 1)), |(c, n)| Some((c, positive(n)?)))?;
                Some(Self::Recording { call: Some((name(call)?, n)), function: name(function)?, tab })
            }
            _ => None,
        }
    }
}

fn positive(part: &str) -> Option<usize> {
    part.parse().ok().filter(|&n| n > 0)
}

/// A piece of a paragraph.
#[derive(Clone, PartialEq, Debug)]
pub enum Piece {
    Text(String),
    Code(String),
    /// [label](target): `to`, what the target names, if it says
    /// (LessonTarget::parse).
    Link { label: String, to: Option<LessonTarget> },
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
            out.push(Piece::Link { label: label.into(), to: LessonTarget::parse(target) });
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
    fn lesson_targets() {
        use LessonTab::*;
        use LessonTarget::*;
        let at = |call: Option<(&str, usize)>, f: &str, tab| Recording { call: call.map(|(c, n)| (c.into(), n)), function: f.into(), tab };
        for (text, target) in [
            ("vfs_read", at(None, "vfs_read", None)),
            ("vfs_read/memory", at(None, "vfs_read", Some(Memory))),
            ("read/vfs_read", at(Some(("read", 1)), "vfs_read", None)),
            ("read#2/ksys_read/stack", at(Some(("read", 2)), "ksys_read", Some(Stack))),
            ("console", Tab(Console)),
            ("try:3", Try { index: 3 }),
        ] {
            assert_eq!(LessonTarget::parse(text), Some(target), "{text}");
        }
    }

    #[test]
    fn invalid_lesson_targets() {
        for text in ["", "/", "a//b", "try:", "try:0", "try:abc", "read#0/f", "read#/f", "a/b/c", "a/b/c/memory", "vfs read"] {
            assert!(LessonTarget::parse(text).is_none(), "{text}");
        }
    }

    #[test]
    fn a_paragraphs_pieces() {
        let p = pieces("1. [vfs_read](vfs_read) reads `buf`; [try](try:1).");
        assert_eq!(p, [
            Piece::Text("1. ".into()),
            Piece::Link { label: "vfs_read".into(),
                          to: Some(LessonTarget::Recording { call: None, function: "vfs_read".into(), tab: None }) },
            Piece::Text(" reads ".into()),
            Piece::Code("buf".into()),
            Piece::Text("; ".into()),
            Piece::Link { label: "try".into(), to: Some(LessonTarget::Try { index: 1 }) },
            Piece::Text(".".into()),
        ]);
    }
}
