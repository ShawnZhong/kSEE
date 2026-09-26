//! A syscall's arguments, written as in C (the page's programs):
//!
//! ```text
//! "a string"              a buffer with the string, NUL-terminated
//! 0644, 0x41, -100        numbers (0: octal, 0x: hex)
//! O_CREAT | O_WRONLY      constants (the kernel's own: Types::constants),
//!                         NULL, names of results (fd, $2), with | << + -
//! [64]                    a buffer of 64 bytes, for the kernel to fill
//! {0, 30000000}           a compound literal, laid out as the parameter's
//! {.tv_nsec = 30000000}   type says (a struct, positional or by name;
//! {"init", NULL}          an array; for a char **, an array of strings)
//! ```

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::{format, vec};

use crate::types::{Base, Kind, Types};

/// An argument's value, as Machine::call takes it.
#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    Number(i64),
    /// A buffer with these bytes (a string: with its NUL).
    Bytes(Vec<u8>),
    /// A buffer of this many zero bytes, for the kernel to fill.
    Size(u32),
    /// An array of char * (None: NULL).
    Strings(Vec<Option<String>>),
}

/// A C constant expression: numbers (decimal, 0x hex, 0 octal, a U or L
/// suffix), names (`known`), `|`, `<<`, `+`, `-`, parentheses.
pub fn eval(s: &str, known: &dyn Fn(&str) -> Option<i64>) -> Result<i64, String> {
    type K<'a> = &'a dyn Fn(&str) -> Option<i64>;
    fn or(t: &mut &str, k: K) -> Result<i64, String> {
        let mut v = shift(t, k)?;
        while let Some(r) = t.trim_start().strip_prefix('|') {
            *t = r;
            v |= shift(t, k)?;
        }
        Ok(v)
    }
    fn shift(t: &mut &str, k: K) -> Result<i64, String> {
        let mut v = sum(t, k)?;
        while let Some(r) = t.trim_start().strip_prefix("<<") {
            *t = r;
            v <<= sum(t, k)?;
        }
        Ok(v)
    }
    fn sum(t: &mut &str, k: K) -> Result<i64, String> {
        let mut v = unary(t, k)?;
        loop {
            let s = t.trim_start();
            if let Some(r) = s.strip_prefix('+') {
                *t = r;
                v += unary(t, k)?;
            } else if let Some(r) = s.strip_prefix('-') {
                *t = r;
                v -= unary(t, k)?;
            } else {
                return Ok(v);
            }
        }
    }
    fn unary(t: &mut &str, k: K) -> Result<i64, String> {
        *t = t.trim_start();
        if let Some(r) = t.strip_prefix('-') {
            *t = r;
            return Ok(-unary(t, k)?);
        }
        if let Some(r) = t.strip_prefix('(') {
            *t = r;
            let v = or(t, k)?;
            *t = t.trim_start().strip_prefix(')').ok_or("a ) missing")?;
            return Ok(v);
        }
        let n = t.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$')).unwrap_or(t.len());
        let (word, rest) = t.split_at(n);
        *t = rest;
        if word.is_empty() {
            return Err(if rest.is_empty() { "a value missing".into() } else { format!("not an argument: {rest}") });
        }
        let num = word.trim_end_matches(['U', 'L', 'u', 'l']);
        if let Some(h) = num.strip_prefix("0x").or_else(|| num.strip_prefix("0X")) {
            i64::from_str_radix(h, 16).map_err(|_| format!("not a number: {word}"))
        } else if num.len() > 1 && num.starts_with('0') {
            i64::from_str_radix(&num[1..], 8).map_err(|_| format!("not a number: {word}"))
        } else if num.starts_with(|c: char| c.is_ascii_digit()) {
            num.parse().map_err(|_| format!("not a number: {word}"))
        } else if word == "NULL" {
            Ok(0)
        } else {
            k(word).ok_or_else(|| format!("not a number, a constant or a result: {word}"))
        }
    }
    let mut t = s;
    let v = or(&mut t, known)?;
    if !t.trim().is_empty() {
        return Err(format!("not an argument: {}", t.trim()));
    }
    Ok(v)
}

/// A compound literal's items: `{a, b}`, `{.x = a}`, nested braces, strings.
#[derive(Debug, Clone, PartialEq)]
enum Item {
    Expr(String),
    Str(String),
    List(Vec<(Option<String>, Item)>),
}

/// The text in the braces of a literal, split at its commas (not those in
/// strings or nested braces), each item with its `.name =` if given.
fn items(text: &str) -> Result<Vec<(Option<String>, Item)>, String> {
    let mut out = Vec::new();
    let (mut depth, mut quote, mut start) = (0, false, 0);
    let bytes = text.as_bytes();
    let mut i = 0;
    let push = |s: &str, out: &mut Vec<(Option<String>, Item)>| -> Result<(), String> {
        let s = s.trim();
        if s.is_empty() {
            return Ok(());
        }
        let (name, value) = match s.strip_prefix('.').and_then(|r| r.split_once('=')) {
            Some((n, v)) if n.trim().chars().all(|c| c.is_ascii_alphanumeric() || c == '_') => (Some(n.trim().to_string()), v.trim()),
            _ => (None, s),
        };
        let item = if let Some(inner) = value.strip_prefix('{').and_then(|v| v.strip_suffix('}')) {
            Item::List(items(inner)?)
        } else if value.starts_with('"') {
            Item::Str(unquote(value)?)
        } else {
            Item::Expr(value.to_string())
        };
        out.push((name, item));
        Ok(())
    };
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if quote => i += 1,
            b'"' => quote = !quote,
            b'{' if !quote => depth += 1,
            b'}' if !quote => depth -= 1,
            b',' if !quote && depth == 0 => {
                push(&text[start..i], &mut out)?;
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    push(&text[start..], &mut out)?;
    Ok(out)
}

/// A C string literal's bytes (\n, \t, \\, \", \0, \xHH).
fn unquote(s: &str) -> Result<String, String> {
    let inner = s.strip_prefix('"').and_then(|r| r.strip_suffix('"')).ok_or_else(|| format!("not a string: {s}"))?;
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('0') => out.push('\0'),
            Some('x') => {
                let h: String = chars.by_ref().take(2).collect();
                out.push(u8::from_str_radix(&h, 16).map_err(|_| format!("bad escape in {s}"))? as char);
            }
            Some(c) => out.push(c),
            None => return Err(format!("bad escape in {s}")),
        }
    }
    Ok(out)
}

impl Types {
    /// Argument `text` for parameter type `ty` (a syscall's, or 0: unknown):
    /// see the module's doc. `known`: the values of names (constants,
    /// results).
    pub fn arg(&self, text: &str, ty: u32, known: &dyn Fn(&str) -> Option<i64>) -> Result<Arg, String> {
        let s = text.trim();
        if s.starts_with('"') {
            let mut b = unquote(s)?.into_bytes();
            b.push(0);
            return Ok(Arg::Bytes(b));
        }
        if let Some(n) = s.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            return Ok(Arg::Size(eval(n, known)? as u32));
        }
        if let Some(inner) = s.strip_prefix('{').and_then(|r| r.strip_suffix('}')) {
            let list = items(inner)?;
            // (The parameter's type: what the braces make.)
            let to = match self.get(self.resolve(ty)).kind {
                Kind::Pointer(to) => self.resolve(to),
                _ if ty == 0 => 0,
                _ => return Err(format!("{{…}} for a {}: not a pointer", self.name(ty))),
            };
            // A char **: an array of strings.
            if matches!(self.get(to).kind, Kind::Pointer(c) if self.is_string(c)) {
                return list.into_iter().map(|(_, it)| match it {
                    Item::Str(s) => Ok(Some(s)),
                    Item::Expr(e) if eval(&e, known)? == 0 => Ok(None),
                    _ => Err("an array of strings: \"…\" or NULL each".to_string()),
                }).collect::<Result<_, _>>().map(Arg::Strings);
            }
            if to == 0 {
                // (Unknown: 64-bit values, as before types said.)
                let mut b = Vec::new();
                for (_, it) in list {
                    let Item::Expr(e) = it else { return Err("a value expected".into()) };
                    b.extend_from_slice(&eval(&e, known)?.to_le_bytes());
                }
                return Ok(Arg::Bytes(b));
            }
            let mut b = vec![0u8; self.size_of(to) as usize];
            self.fill(&mut b, 0, to, &Item::List(list), known)?;
            return Ok(Arg::Bytes(b));
        }
        Ok(Arg::Number(eval(s, known)?))
    }

    /// Put `item` into `buf` at `at`, as a value of type `ty`.
    fn fill(&self, buf: &mut Vec<u8>, at: usize, ty: u32, item: &Item, known: &dyn Fn(&str) -> Option<i64>) -> Result<(), String> {
        let id = self.resolve(ty);
        let t = self.get(id);
        match (&t.kind, item) {
            (Kind::Struct(_) | Kind::Union(_), Item::List(list)) => {
                let members = self.members(&t.kind);
                let mut next = 0;
                for (name, it) in list {
                    let k = match name {
                        Some(n) => members.iter().position(|m| &m.name == n).ok_or_else(|| format!("{} has no member {n}", self.name(id)))?,
                        None => next,
                    };
                    let m = members.get(k).ok_or_else(|| format!("{}: too many values", self.name(id)))?;
                    if m.bit_size > 0 {
                        return Err(format!("{}.{}: a bitfield", self.name(id), m.name));
                    }
                    self.fill(buf, at + (m.bit_offset / 8) as usize, m.ty, it, known)?;
                    next = k + 1;
                }
                Ok(())
            }
            (Kind::Array { elem, .. }, Item::Str(s)) if self.is_string(*elem) => {
                let b = s.as_bytes();
                let room = self.size_of(id) as usize;
                if b.len() + 1 > room {
                    return Err(format!("\"{s}\": longer than {room} bytes"));
                }
                buf[at..at + b.len()].copy_from_slice(b);
                Ok(())
            }
            (Kind::Array { elem, count }, Item::List(list)) => {
                if list.len() > *count as usize {
                    return Err(format!("{}: too many values", self.name(id)));
                }
                let esize = self.size_of(*elem) as usize;
                for (i, (_, it)) in list.iter().enumerate() {
                    self.fill(buf, at + i * esize, *elem, it, known)?;
                }
                Ok(())
            }
            (Kind::Base(_) | Kind::Enum(_) | Kind::Pointer(_), Item::Expr(e)) => {
                let v = eval(e, known)?;
                let size = self.size_of(id).clamp(1, 8) as usize;
                if matches!(t.kind, Kind::Base(Base::Float)) {
                    return Err(format!("{}: a float", self.name(id)));
                }
                buf[at..at + size].copy_from_slice(&v.to_le_bytes()[..size]);
                Ok(())
            }
            (_, Item::Str(s)) => Err(format!("\"{s}\" for a {}", self.name(id))),
            (_, Item::List(_)) => Err(format!("{{…}} for a {}", self.name(id))),
            (_, Item::Expr(e)) => Err(format!("{e} for a {}", self.name(id))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TYPES: &str = "T\t1\tbase\tlong long\t8\ts\nT\t2\tstruct\ttimespec\t16\nM\t2\ttv_sec\t0\t0\t1\nM\t2\ttv_nsec\t64\t0\t1\n\
                         T\t3\tptr\t\t4\t2\nT\t4\tbase\tchar\t1\tc\nT\t5\tptr\t\t4\t4\nT\t6\tptr\t\t4\t5\nT\t7\tbase\tint\t4\ts\n\
                         T\t8\tarray\t\t8\t4\t8\nT\t9\tstruct\tnamed\t12\nM\t9\tn\t0\t0\t7\nM\t9\ts\t32\t0\t8\nT\t10\tptr\t\t4\t9\n\
                         T\t11\tarray\t\t8\t7\t2\nT\t12\tstruct\tpair\t8\nM\t12\tv\t0\t0\t11\nT\t13\tptr\t\t4\t12\n";

    fn known(name: &str) -> Option<i64> {
        [("O_CREAT", 64), ("O_WRONLY", 1), ("AT_FDCWD", -100), ("fd", 3), ("$2", 5)].iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
    }

    #[test]
    fn expressions() {
        assert_eq!(eval("O_CREAT | O_WRONLY", &known), Ok(65));
        assert_eq!(eval("0644", &known), Ok(420));
        assert_eq!(eval("(1 << 4) + fd - $2", &known), Ok(14));
        assert_eq!(eval("NULL", &known), Ok(0));
        assert!(eval("nope", &known).is_err());
        assert!(eval("1 2", &known).is_err());
    }

    #[test]
    fn a_struct_is_laid_out_by_its_type() {
        let t = Types::parse(TYPES);
        let ts = |text| t.arg(text, 3, &known);
        let want = Arg::Bytes([0u64.to_le_bytes(), 30000000u64.to_le_bytes()].concat());
        assert_eq!(ts("{0, 30000000}"), Ok(want.clone()));
        assert_eq!(ts("{.tv_nsec = 30000000}"), Ok(want));
        assert_eq!(ts("{.tv_sec = 1}"), Ok(Arg::Bytes([1u64.to_le_bytes(), [0; 8]].concat())));
        assert!(ts("{.tv_min = 1}").unwrap_err().contains("no member"));
        assert!(ts("{1, 2, 3}").unwrap_err().contains("too many"));
        // A struct with an int and a char array; one with an array member.
        let mut named = vec![0u8; 12];
        named[0] = 7;
        named[4..7].copy_from_slice(b"abc");
        assert_eq!(t.arg("{7, \"abc\"}", 10, &known), Ok(Arg::Bytes(named)));
        assert_eq!(t.arg("{{1, 2}}", 13, &known), Ok(Arg::Bytes([1u32.to_le_bytes(), 2u32.to_le_bytes()].concat())));
    }

    #[test]
    fn strings_buffers_and_argv() {
        let t = Types::parse(TYPES);
        assert_eq!(t.arg("\"a\\n\"", 5, &known), Ok(Arg::Bytes(b"a\n\0".to_vec())));
        assert_eq!(t.arg("[64]", 5, &known), Ok(Arg::Size(64)));
        assert_eq!(t.arg("{\"init\", NULL}", 6, &known), Ok(Arg::Strings(vec![Some("init".into()), None])));
        assert_eq!(t.arg("fd", 7, &known), Ok(Arg::Number(3)));
        // (No type known: 64-bit values.)
        assert_eq!(t.arg("{1, 2}", 0, &known), Ok(Arg::Bytes([1u64.to_le_bytes(), 2u64.to_le_bytes()].concat())));
        assert!(t.arg("{1}", 7, &known).unwrap_err().contains("not a pointer"));
    }
}
