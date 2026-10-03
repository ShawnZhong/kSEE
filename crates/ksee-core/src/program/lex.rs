//! A program's text as C's tokens, each with where it is (bytes), for the
//! parser and the editor.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Tok {
    Num(i64),
    Name(String),
    /// A string literal's bytes (no NUL added).
    Str(Vec<u8>),
    Op(&'static str),
}

/// A token and its bytes in the text, `start..end`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Token {
    pub tok: Tok,
    pub start: usize,
    pub end: usize,
}

const OPS: [&str; 19] = ["<<", ">>", "(", ")", "[", "]", "{", "}", ",", ";", "=", "*", "/",
                         "+", "-", "~", "&", "|", "^"];

/// `text`'s tokens. (A string not closed: an error.)
pub(crate) fn tokens(text: &str) -> Result<Vec<Token>, String> {
    let mut out = Vec::new();
    let mut i = 0;
    let b = text.as_bytes();
    while i < b.len() {
        let c = b[i];
        let start = i;
        let tok = if c.is_ascii_whitespace() {
            i += 1;
            continue;
        } else if c.is_ascii_digit() {
            while i < b.len() && b[i].is_ascii_alphanumeric() {
                i += 1;
            }
            let word = &text[start..i];
            let num = word.trim_end_matches(['U', 'L', 'u', 'l']);
            let v = if let Some(h) = num.strip_prefix("0x").or_else(|| num.strip_prefix("0X")) {
                i64::from_str_radix(h, 16)
            } else if num.len() > 1 && num.starts_with('0') {
                i64::from_str_radix(&num[1..], 8)
            } else {
                num.parse()
            };
            Tok::Num(v.map_err(|_| format!("not a number: {word}"))?)
        } else if c.is_ascii_alphabetic() || c == b'_' {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            Tok::Name(text[start..i].to_string())
        } else if c == b'"' {
            // (A string: to its closing quote, past escapes.)
            i += 1;
            while i < b.len() && b[i] != b'"' {
                i += if b[i] == b'\\' { 2 } else { 1 };
            }
            if i >= b.len() {
                return Err("a \" missing".into());
            }
            i += 1;
            Tok::Str(unquote(&text[start + 1..i - 1])?)
        } else {
            let op = OPS.iter().find(|o| text[i..].starts_with(**o)).ok_or_else(|| format!("not C: {}", &text[i..]))?;
            i += op.len();
            Tok::Op(op)
        };
        out.push(Token { tok, start, end: i });
    }
    Ok(out)
}

/// A C string literal's inside as bytes (\n, \t, \r, \\, \", \0, \xHH).
pub(crate) fn unquote(inner: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut chars = inner.chars();
    // (A character as its UTF-8 bytes; an escape, the byte it says.)
    let push = |out: &mut Vec<u8>, c: char| out.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
    while let Some(c) = chars.next() {
        if c != '\\' {
            push(&mut out, c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push(b'\n'),
            Some('t') => out.push(b'\t'),
            Some('r') => out.push(b'\r'),
            Some('0') => out.push(0),
            Some('x') => {
                let h: String = chars.by_ref().take(2).collect();
                out.push(u8::from_str_radix(&h, 16).map_err(|_| format!("bad escape: \\x{h}"))?);
            }
            Some(c) => push(&mut out, c),
            None => return Err("a \\ at the end".into()),
        }
    }
    Ok(out)
}
