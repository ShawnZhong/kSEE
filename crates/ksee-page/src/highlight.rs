//! C source as colored pieces, a line at a time (comments that span lines
//! carried over): keywords, types, strings and characters, comments,
//! numbers, preprocessor lines, and names (which may have values).

/// A piece of a line, and what it is.
#[derive(Clone, PartialEq, Debug)]
pub struct Piece {
    pub text: String,
    pub kind: Kind,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Plain,
    Keyword,
    Type,
    Str,
    Comment,
    Number,
    Pre,
    Name,
}

impl Kind {
    /// Its CSS class.
    pub fn class(self) -> &'static str {
        match self {
            Kind::Plain => "",
            Kind::Keyword => "kw",
            Kind::Type => "ty",
            Kind::Str => "str",
            Kind::Comment => "com",
            Kind::Number => "num",
            Kind::Pre => "pp",
            Kind::Name => "nm",
        }
    }
}

const KEYWORDS: &[&str] = &["if", "else", "for", "while", "do", "switch", "case", "default", "break", "continue", "return", "goto",
                            "sizeof", "typeof", "static", "inline", "extern", "const", "volatile", "register", "restrict",
                            "struct", "union", "enum", "typedef", "__always_inline", "__must_check", "noinline", "asm"];
const TYPES: &[&str] = &["void", "char", "short", "int", "long", "unsigned", "signed", "bool", "float", "double", "size_t", "ssize_t",
                         "loff_t", "u8", "u16", "u32", "u64", "s8", "s16", "s32", "s64", "__u32", "__u64", "umode_t", "pid_t", "uid_t", "gid_t"];

/// The lines of a file, each as pieces.
pub fn file(lines: &[String]) -> Vec<Vec<Piece>> {
    let mut in_comment = false;
    lines.iter().map(|l| line(l, &mut in_comment)).collect()
}

fn line(text: &str, in_comment: &mut bool) -> Vec<Piece> {
    let mut out: Vec<Piece> = Vec::new();
    let push = |out: &mut Vec<Piece>, text: &str, kind: Kind| {
        match out.last_mut() {
            Some(p) if p.kind == kind && kind != Kind::Name => p.text.push_str(text),
            _ => out.push(Piece { text: text.into(), kind }),
        }
    };
    let b = text.as_bytes();
    let mut i = 0;
    if !*in_comment && text.trim_start().starts_with('#') {
        return vec![Piece { text: text.into(), kind: Kind::Pre }];
    }
    while i < b.len() {
        if *in_comment {
            let end = text[i..].find("*/").map_or(b.len(), |k| i + k + 2);
            *in_comment = end == b.len() && !text[i..].contains("*/");
            push(&mut out, &text[i..end], Kind::Comment);
            i = end;
            continue;
        }
        let c = b[i];
        let start = i;
        if text[i..].starts_with("//") {
            push(&mut out, &text[i..], Kind::Comment);
            break;
        } else if text[i..].starts_with("/*") {
            *in_comment = true;
            continue;
        } else if c == b'"' || c == b'\'' {
            i += 1;
            while i < b.len() && b[i] != c {
                i += if b[i] == b'\\' { 2 } else { 1 };
            }
            i = (i + 1).min(b.len());
            push(&mut out, &text[start..i], Kind::Str);
        } else if c.is_ascii_digit() {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'.') {
                i += 1;
            }
            push(&mut out, &text[start..i], Kind::Number);
        } else if c.is_ascii_alphabetic() || c == b'_' {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            let w = &text[start..i];
            let kind = if KEYWORDS.contains(&w) { Kind::Keyword } else if TYPES.contains(&w) { Kind::Type } else { Kind::Name };
            push(&mut out, w, kind);
        } else {
            let n = text[i..].chars().next().map_or(1, char::len_utf8);
            i += n;
            push(&mut out, &text[start..i], Kind::Plain);
        }
    }
    out
}

/// The expression a name piece ends, with what is before it on its line:
/// `file`, `file->f_pos`, `a.b->c` (pieces `k` back over `.` and `->`).
pub fn expr(pieces: &[Piece], k: usize) -> String {
    let mut parts = vec![pieces[k].text.clone()];
    let mut j = k;
    while j >= 2 && matches!(pieces[j - 1].text.trim(), "." | "->") && pieces[j - 2].kind == Kind::Name {
        parts.push(pieces[j - 1].text.trim().into());
        parts.push(pieces[j - 2].text.clone());
        j -= 2;
    }
    parts.reverse();
    parts.concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lines_pieces() {
        let mut c = false;
        let p = line("\tif (file->f_mode & FMODE_READ) /* x", &mut c);
        let kinds: Vec<(&str, Kind)> = p.iter().map(|p| (p.text.as_str(), p.kind)).collect();
        assert_eq!(kinds[1], ("if", Kind::Keyword));
        assert!(kinds.contains(&("file", Kind::Name)) && kinds.contains(&("f_mode", Kind::Name)));
        assert!(c, "a comment goes on");
        assert_eq!(line("still */ x = 1;", &mut c)[0].kind, Kind::Comment);
        assert!(!c);
        let p = line("x = a.b->c;", &mut c);
        let k = p.iter().position(|p| p.text == "c").unwrap();
        assert_eq!(expr(&p, k), "a.b->c");
    }
}
