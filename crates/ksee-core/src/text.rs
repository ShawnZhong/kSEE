//! A lesson's paragraphs: text with `code` and [links](target) into the
//! recording.

use alloc::string::String;
use alloc::vec::Vec;

/// A panel a lesson link opens.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LessonTab {
    Stack,
    Objects,
    Tasks,
    Console,
}

impl LessonTab {
    fn parse(part: &str) -> Option<Self> {
        match part {
            "stack" => Some(Self::Stack),
            "objects" => Some(Self::Objects),
            "tasks" => Some(Self::Tasks),
            "console" => Some(Self::Console),
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

/// Why a lesson-link target is not part of the lesson grammar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LessonTargetError {
    InvalidIndex,
    ZeroIndex,
    EmptyPart,
    UnexpectedPart,
}

impl LessonTarget {
    /// Parse `N`, `N:function[:any][:tab]`, `N:tab`, or `try:N`.
    pub fn parse(target: &str) -> Result<Self, LessonTargetError> {
        let mut parts = target.split(':');
        let first = parts.next().ok_or(LessonTargetError::InvalidIndex)?;
        if first == "try" {
            let index = positive(parts.next().ok_or(LessonTargetError::InvalidIndex)?)?;
            return if parts.next().is_none() {
                Ok(Self::Try { index })
            } else {
                Err(LessonTargetError::UnexpectedPart)
            };
        }

        let call = positive(first)?;
        let Some(second) = parts.next() else {
            return Ok(Self::Recording { call, function: None, any_task: false, tab: None });
        };
        if second.is_empty() {
            return Err(LessonTargetError::EmptyPart);
        }
        if let Some(tab) = LessonTab::parse(second) {
            return if parts.next().is_none() {
                Ok(Self::Recording { call, function: None, any_task: false, tab: Some(tab) })
            } else {
                Err(LessonTargetError::UnexpectedPart)
            };
        }
        if second == "any" {
            return Err(LessonTargetError::UnexpectedPart);
        }

        let function = Some(second.into());
        let mut any_task = false;
        let mut tab = None;
        if let Some(third) = parts.next() {
            if third == "any" {
                any_task = true;
                if let Some(fourth) = parts.next() {
                    tab = LessonTab::parse(fourth).ok_or(LessonTargetError::UnexpectedPart).map(Some)?;
                }
            } else {
                tab = LessonTab::parse(third).ok_or(LessonTargetError::UnexpectedPart).map(Some)?;
            }
        }
        if parts.next().is_some() {
            return Err(LessonTargetError::UnexpectedPart);
        }
        Ok(Self::Recording { call, function, any_task, tab })
    }

    pub fn call(&self) -> Option<usize> {
        match self {
            Self::Try { .. } => None,
            Self::Recording { call, .. } => Some(*call),
        }
    }
}

fn positive(part: &str) -> Result<usize, LessonTargetError> {
    if part.is_empty() {
        return Err(LessonTargetError::EmptyPart);
    }
    match part.parse() {
        Ok(0) => Err(LessonTargetError::ZeroIndex),
        Ok(n) => Ok(n),
        Err(_) => Err(LessonTargetError::InvalidIndex),
    }
}

/// A piece of a paragraph.
#[derive(Clone, PartialEq, Debug)]
pub enum Piece {
    Text(String),
    Code(String),
    /// [label](target): `call`, the recording call it names, if valid.
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
            let call = LessonTarget::parse(target).ok().and_then(|target| target.call());
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
    fn lesson_targets() {
        use LessonTab::*;
        use LessonTarget::*;
        for (text, target) in [
            ("1", Recording { call: 1, function: None, any_task: false, tab: None }),
            ("2:vfs_read", Recording { call: 2, function: Some("vfs_read".into()), any_task: false, tab: None }),
            ("2:vfs_read:any", Recording { call: 2, function: Some("vfs_read".into()), any_task: true, tab: None }),
            ("2:objects", Recording { call: 2, function: None, any_task: false, tab: Some(Objects) }),
            ("2:vfs_read:stack", Recording { call: 2, function: Some("vfs_read".into()), any_task: false, tab: Some(Stack) }),
            ("2:vfs_read:any:tasks", Recording { call: 2, function: Some("vfs_read".into()), any_task: true, tab: Some(Tasks) }),
            ("2:console", Recording { call: 2, function: None, any_task: false, tab: Some(Console) }),
            ("try:3", Try { index: 3 }),
        ] {
            assert_eq!(LessonTarget::parse(text), Ok(target), "{text}");
        }
    }

    #[test]
    fn invalid_lesson_targets() {
        for text in ["", "abc", "0", ":vfs_read", "2:", "try", "try:", "try:0", "try:abc",
                     "try:1:stack", "2:any", "2:vfs_read:unknown", "2:vfs_read:stack:objects",
                     "2:vfs_read:any:any", "2:vfs_read:any:stack:extra"] {
            assert!(LessonTarget::parse(text).is_err(), "{text}");
        }
    }

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
