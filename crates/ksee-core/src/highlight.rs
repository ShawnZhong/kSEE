//! C source as colored pieces, a line at a time (a comment or a macro that
//! spans lines carried over): keywords, types (the common ones, a name
//! after struct, union or enum, one ending in _t), calls (a name before `(`;
//! a definition too: the cheap rule colors both the same, as CodeMirror's
//! cpp() does in the dark), fields (a name after `.` or `->`, unless it is
//! a call), macros and constants (all capitals), NULL/true/false, strings
//! and characters (their escapes their own pieces), comments, numbers,
//! preprocessor lines, and names (which may have values).
//!
//! Every identifier (names, calls, fields, macros) hovers: [`Class::is_ident`]
//! tells the page so.

use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;

/// A piece of a line, and what it is.
#[derive(Clone, PartialEq, Debug)]
pub struct Piece {
    pub text: String,
    pub kind: Class,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Class {
    Plain,
    Keyword,
    Type,
    Str,
    Esc,
    Comment,
    Number,
    Pre,
    Name,
    Func,
    Field,
    Macro,
    Bool,
}

impl Class {
    /// Whether it is an identifier the page hovers (a name, a call, a
    /// field, a macro: everything [`expr`] walks over).
    pub fn is_ident(self) -> bool {
        matches!(self, Class::Name | Class::Func | Class::Field | Class::Macro)
    }
}

const KEYWORDS: &[&str] = &["if", "else", "for", "while", "do", "switch", "case", "default", "break", "continue", "return", "goto",
                            "sizeof", "typeof", "static", "inline", "extern", "const", "volatile", "register", "restrict",
                            "struct", "union", "enum", "typedef", "__always_inline", "__must_check", "noinline", "asm"];
const TYPES: &[&str] = &["void", "char", "short", "int", "long", "unsigned", "signed", "bool", "float", "double", "size_t", "ssize_t",
                         "loff_t", "u8", "u16", "u32", "u64", "s8", "s16", "s32", "s64", "__u32", "__u64", "umode_t", "pid_t", "uid_t", "gid_t"];
/// NULL/true/false (lezer's null, a keyword's child, and bool, a literal's:
/// the cpp mode colors the booleans; NULL goes with them, usefully).
const BOOLS: &[&str] = &["NULL", "nullptr", "true", "false", "TRUE", "FALSE"];

/// Whether a word is a macro or constant (all capitals: FMODE_READ,
/// MAX_RW_COUNT, EBADF; longer than one, so `x` stays a name; not a boolean:
/// NULL and TRUE are [`BOOLS`]).
fn is_macro(w: &str) -> bool {
    w.len() > 1 && w.bytes().all(|c: u8| !c.is_ascii_lowercase())
        && w.bytes().any(|c: u8| c.is_ascii_uppercase())
}

/// What a line leaves open for the next: a comment, a macro (a line
/// ending in a backslash).
#[derive(Default)]
struct Open {
    comment: bool,
    macro_: bool,
}

/// The lines of a file, each as pieces.
pub fn file(lines: &[String]) -> Vec<Vec<Piece>> {
    let mut open = Open::default();
    lines.iter().map(|l| line(l, &mut open)).collect()
}

fn line(text: &str, open: &mut Open) -> Vec<Piece> {
    let in_comment = &mut open.comment;
    let mut out: Vec<Piece> = Vec::new();
    // (One piece an identifier: merging would join two names past the
    // whitespace between them, and the page hovers each on its own.)
    let push = |out: &mut Vec<Piece>, text: &str, kind: Class| {
        match out.last_mut() {
            Some(p) if p.kind == kind && !kind.is_ident() => p.text.push_str(text),
            _ => out.push(Piece { text: text.into(), kind }),
        }
    };
    let b = text.as_bytes();
    let mut i = 0;
    if open.macro_ || (!*in_comment && text.trim_start().starts_with('#')) {
        open.macro_ = text.trim_end().ends_with('\\');
        return vec![Piece { text: text.into(), kind: Class::Pre }];
    }
    // (The last word: struct, union or enum makes the next a type; `.` or
    // `->`, a field.)
    let mut last = "";
    while i < b.len() {
        // (A comment, to its end: on this line or a later one; `/*/` only opens one.)
        if *in_comment || text[i..].starts_with("/*") {
            let from = if *in_comment { i } else { i + 2 };
            let end = text[from..].find("*/").map(|k| from + k + 2);
            *in_comment = end.is_none();
            let end = end.unwrap_or(b.len());
            push(&mut out, &text[i..end], Class::Comment);
            i = end;
            continue;
        }
        let c = b[i];
        let start = i;
        if text[i..].starts_with("//") {
            push(&mut out, &text[i..], Class::Comment);
            break;
        } else if c == b'"' || c == b'\'' {
            push(&mut out, &text[start..start + 1], Class::Str);
            i += 1;
            let mut seg = start + 1;
            while i < b.len() && b[i] != c {
                if b[i] == b'\\' {
                    push(&mut out, &text[seg..i], Class::Str);
                    let end = (i + 2).min(b.len());
                    push(&mut out, &text[i..end], Class::Esc);
                    i = end;
                    seg = i;
                } else {
                    i += 1;
                }
            }
            push(&mut out, &text[seg..i], Class::Str);
            if i < b.len() {
                push(&mut out, &text[i..i + 1], Class::Str);
                i += 1;
            }
        } else if c.is_ascii_digit() || (c == b'.' && text[i + 1..].starts_with(|c: char| c.is_ascii_digit())) {
            i += 1;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'.') {
                i += 1;
            }
            push(&mut out, &text[start..i], Class::Number);
        } else if c.is_ascii_alphabetic() || c == b'_' {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            let w = &text[start..i];
            // (A call wins over the `_t` guess and over a field: `min_t(`
            // and `file->f_op->read(` call, as CodeMirror's cpp() has it.)
            let after = text[i..].trim_start();
            let field = matches!(last.trim(), "." | "->");
            let kind = if KEYWORDS.contains(&w) {
                Class::Keyword
            } else if BOOLS.contains(&w) {
                Class::Bool
            } else if after.starts_with('(') {
                Class::Func
            } else if TYPES.contains(&w) || w.ends_with("_t") || matches!(last, "struct" | "union" | "enum") {
                Class::Type
            } else if field {
                Class::Field
            } else if is_macro(w) {
                Class::Macro
            } else {
                Class::Name
            };
            push(&mut out, w, kind);
            last = w;
            continue;
        } else {
            // (`->` one piece, so [`expr`] walks over it; `*` at a line's
            // start a pointer, not times.)
            if text[i..].starts_with("->") {
                push(&mut out, "->", Class::Plain);
                i += 2;
            } else {
                let n = text[i..].chars().next().map_or(1, char::len_utf8);
                i += n;
                push(&mut out, &text[start..i], Class::Plain);
            }
        }
        if !text[start..i].trim().is_empty() {
            // (`.` and `->` stay, so the name after them is a field;
            // anything else clears the last word.)
            last = match &text[start..i] {
                "->" | "." => &text[start..i],
                _ => "",
            };
        }
    }
    out
}

/// The expression an identifier piece ends, with what is before it on its
/// line: `file`, `file->f_pos`, `a.b->c` (pieces `k` back over `.` and `->`;
/// every identifier kind, so calls and fields hover too).
pub fn expr(pieces: &[Piece], k: usize) -> String {
    let mut parts = vec![pieces[k].text.clone()];
    let mut j = k;
    while j >= 2 && matches!(pieces[j - 1].text.trim(), "." | "->") && pieces[j - 2].kind.is_ident() {
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

    fn kind_of(p: &[Piece]) -> Vec<Class> {
        p.iter().map(|p| p.kind).collect()
    }

    #[test]
    fn a_lines_pieces() {
        let mut c = Open::default();
        let p = line("\tif (file->f_mode & FMODE_READ) /* x", &mut c);
        let kinds: Vec<(&str, Class)> = p.iter().map(|p| (p.text.as_str(), p.kind)).collect();
        assert_eq!(kinds[1], ("if", Class::Keyword));
        assert!(kinds.contains(&("file", Class::Name)) && kinds.contains(&("f_mode", Class::Field)));
        assert!(kinds.contains(&("FMODE_READ", Class::Macro)));
        assert!(c.comment, "a comment goes on");
        assert_eq!(line("still */ x = 1;", &mut c)[0].kind, Class::Comment);
        assert!(!c.comment);
        // (`/*/` opens a comment, not closes it; one closed on its line ends there.)
        line("a /*/ b", &mut c);
        assert!(c.comment);
        line("*/", &mut c);
        assert_eq!(kind_of(&line("/* a */ x .5", &mut c)), [Class::Comment, Class::Plain, Class::Name, Class::Plain, Class::Number]);
        let p = line("x = a.b->c;", &mut c);
        let k = p.iter().position(|p| p.text == "c").unwrap();
        assert_eq!(expr(&p, k), "a.b->c");
        // (Types: after struct; ending in _t. A macro goes on past a backslash.)
        let kind = |l: &str, w: &str, c: &mut Open| line(l, c).into_iter().find(|p| p.text == w).map(|p| p.kind);
        assert_eq!(kind("struct file *f;", "file", &mut c), Some(Class::Type));
        assert_eq!(kind("struct file *f;", "f", &mut c), Some(Class::Name));
        assert_eq!(kind("gfp_t gfp = 0;", "gfp_t", &mut c), Some(Class::Type));
        line("#define X(a) \\", &mut c);
        assert_eq!(line("\t(a + 1)", &mut c)[0].kind, Class::Pre);
        assert_eq!(line("int y;", &mut c)[0].kind, Class::Type);
    }

    /// Real kernel lines (fs/read_write.c): calls, definitions, fields,
    /// macros, booleans, escapes; every identifier still hoverable.
    #[test]
    fn kernel_lines() {
        let mut c = Open::default();
        let kind = |l: &str, w: &str, c: &mut Open| line(l, c).into_iter().find(|p| p.text == w).map(|p| p.kind);
        // (A definition colors like a call: the cheap rule, as CodeMirror's
        // cpp() does in the dark.)
        assert_eq!(kind("ssize_t ksys_read(unsigned int fd, char __user *buf, size_t count)", "ksys_read", &mut c), Some(Class::Func));
        assert_eq!(kind("static inline loff_t *file_ppos(struct file *file)", "file_ppos", &mut c), Some(Class::Func));
        assert_eq!(kind("\treturn file->f_mode & FMODE_STREAM ? NULL : &file->f_pos;", "f_mode", &mut c), Some(Class::Field));
        assert_eq!(kind("\treturn file->f_mode & FMODE_STREAM ? NULL : &file->f_pos;", "FMODE_STREAM", &mut c), Some(Class::Macro));
        assert_eq!(kind("\treturn file->f_mode & FMODE_STREAM ? NULL : &file->f_pos;", "NULL", &mut c), Some(Class::Bool));
        assert_eq!(kind("\t\tret = vfs_read(fd_file(f), buf, count, ppos);", "vfs_read", &mut c), Some(Class::Func));
        assert_eq!(kind("\t\tret = vfs_read(fd_file(f), buf, count, ppos);", "fd_file", &mut c), Some(Class::Func));
        assert_eq!(kind("\tif (WARN_ON_ONCE(!(file->f_mode & FMODE_READ)))", "WARN_ON_ONCE", &mut c), Some(Class::Func));
        assert_eq!(kind("\tif (WARN_ON_ONCE(!(file->f_mode & FMODE_READ)))", "FMODE_READ", &mut c), Some(Class::Macro));
        assert_eq!(kind("\t\t.iov_len\t= min_t(size_t, count, MAX_RW_COUNT),", "min_t", &mut c), Some(Class::Func));
        assert_eq!(kind("\t\t.iov_len\t= min_t(size_t, count, MAX_RW_COUNT),", "MAX_RW_COUNT", &mut c), Some(Class::Macro));
        assert_eq!(kind("\t\treturn -EBADF;", "EBADF", &mut c), Some(Class::Macro));
        assert_eq!(kind("\t\t\tfsnotify_modify(file);", "fsnotify_modify", &mut c), Some(Class::Func));
        // (A field call: the call wins, as CodeMirror's cpp() has it.)
        assert_eq!(kind("\t\tret = file->f_op->read(file, buf, count, pos);", "read", &mut c), Some(Class::Func));
        assert_eq!(kind("\t\tret = file->f_op->read(file, buf, count, pos);", "f_op", &mut c), Some(Class::Field));
        // (Escapes are their own pieces, in strings and characters.)
        let p = line("\t\t\"kernel %s (pid: %d)\\n\",", &mut c);
        assert_eq!(p.iter().filter(|p| p.kind == Class::Esc).map(|p| p.text.as_str()).collect::<Vec<_>>(), ["\\n"]);
        let p = line("\tchar c = '\\'';", &mut c);
        assert!(p.iter().any(|p| p.kind == Class::Esc && p.text == "\\'"));
        // (true/false, and the single letters that must stay names.)
        assert_eq!(kind("\tbool empty = true;", "true", &mut c), Some(Class::Bool));
        assert_eq!(kind("\tif (!fd_empty(f)) {", "fd_empty", &mut c), Some(Class::Func));
        let p = line("\tloff_t pos, *ppos = file_ppos(fd_file(f));", &mut c);
        assert_eq!(p.iter().find(|p| p.text == "pos").map(|p| p.kind), Some(Class::Name));
        // (Every identifier hovers: names, calls, fields, macros; booleans
        // hover too, by kind, though they have no value.)
        for (l, w) in [("ssize_t ksys_read(unsigned int fd);", "ksys_read"),
                       ("\t\tret = vfs_read(fd_file(f), buf, count, ppos);", "vfs_read"),
                       ("\treturn file->f_mode & FMODE_STREAM ? NULL : &file->f_pos;", "file"),
                       ("\treturn file->f_mode & FMODE_STREAM ? NULL : &file->f_pos;", "f_mode"),
                       ("\treturn file->f_mode & FMODE_STREAM ? NULL : &file->f_pos;", "FMODE_STREAM"),
                       ("\tloff_t pos, *ppos = file_ppos(fd_file(f));", "pos")] {
            let p = line(l, &mut c);
            let k = p.iter().position(|p| p.text == w).unwrap_or_else(|| panic!("{w} pieces"));
            assert!(p[k].kind.is_ident(), "{w} hovers");
        }
        assert!(!Class::Bool.is_ident());
        assert!(!Class::Plain.is_ident());
        // (An expression over the new kinds still walks back to its base.)
        let p = line("\tret = file->f_op->read(file, buf, count, pos);", &mut c);
        let k = p.iter().position(|p| p.text == "f_op").unwrap();
        assert_eq!(expr(&p, k), "file->f_op");
    }
}
