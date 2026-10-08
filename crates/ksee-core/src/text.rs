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
    Try { index: usize },
    Recording {
        call: usize,
        function: Option<String>,
        any_task: bool,
        tab: Option<LessonTab>,
    },
}

impl LessonTarget {
    /// Parse `N`, `N:function[:any][:tab]`, `N:tab`, or `try:N`.
    pub fn parse(target: &str) -> Option<Self> {
        let mut parts = target.split(':');
        let first = parts.next()?;
        if first == "try" {
            let index = positive(parts.next()?)?;
            return parts.next().is_none().then_some(Self::Try { index });
        }
        let call = positive(first)?;
        let Some(second) = parts.next() else {
            return Some(Self::Recording { call, function: None, any_task: false, tab: None });
        };
        if let Some(tab) = LessonTab::parse(second) {
            return parts.next().is_none().then_some(Self::Recording { call, function: None, any_task: false, tab: Some(tab) });
        }
        if second.is_empty() || second == "any" {
            return None;
        }
        let mut next = parts.next();
        let any_task = next == Some("any");
        if any_task {
            next = parts.next();
        }
        let tab = match next {
            Some(t) => Some(LessonTab::parse(t)?),
            None => None,
        };
        parts.next().is_none().then_some(Self::Recording { call, function: Some(second.into()), any_task, tab })
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
        for (text, target) in [
            ("1", Recording { call: 1, function: None, any_task: false, tab: None }),
            ("2:vfs_read", Recording { call: 2, function: Some("vfs_read".into()), any_task: false, tab: None }),
            ("2:vfs_read:any", Recording { call: 2, function: Some("vfs_read".into()), any_task: true, tab: None }),
            ("2:memory", Recording { call: 2, function: None, any_task: false, tab: Some(Memory) }),
            ("2:vfs_read:stack", Recording { call: 2, function: Some("vfs_read".into()), any_task: false, tab: Some(Stack) }),
            ("2:vfs_read:any:tasks", Recording { call: 2, function: Some("vfs_read".into()), any_task: true, tab: Some(Tasks) }),
            ("2:console", Recording { call: 2, function: None, any_task: false, tab: Some(Console) }),
            ("try:3", Try { index: 3 }),
        ] {
            assert_eq!(LessonTarget::parse(text), Some(target), "{text}");
        }
    }

    #[test]
    fn invalid_lesson_targets() {
        for text in ["", "abc", "0", ":vfs_read", "2:", "try", "try:", "try:0", "try:abc",
                     "try:1:stack", "2:any", "2:vfs_read:unknown", "2:vfs_read:stack:memory",
                     "2:vfs_read:any:any", "2:vfs_read:any:stack:extra"] {
            assert!(LessonTarget::parse(text).is_none(), "{text}");
        }
    }

    #[test]
    fn a_paragraphs_pieces() {
        let p = pieces("1. [vfs_read](2:vfs_read) reads `buf`; [try](try:1).");
        assert_eq!(p, [
            Piece::Text("1. ".into()),
            Piece::Link { label: "vfs_read".into(),
                          to: Some(LessonTarget::Recording { call: 2, function: Some("vfs_read".into()), any_task: false, tab: None }) },
            Piece::Text(" reads ".into()),
            Piece::Code("buf".into()),
            Piece::Text("; ".into()),
            Piece::Link { label: "try".into(), to: Some(LessonTarget::Try { index: 1 }) },
            Piece::Text(".".into()),
        ]);
    }
}
