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
use crate::Memory;

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
/// suffix), names (`known`, `&name` the same; `name[i]` known's as
/// "name[i]": a variable's element), `|`, `<<`, `+`, `-`, parentheses.
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
        // ('h', '\n': a character's number.)
        if let Some(r) = t.strip_prefix('\'') {
            let end = r.char_indices().skip(1).find(|&(i, c)| c == '\'' && !r[..i].ends_with('\\')).map(|(i, _)| i)
                .or_else(|| r.find('\''))
                .ok_or("a ' missing")?;
            let c = unquote(&format!("\"{}\"", &r[..end]))?;
            *t = &r[end + 1..];
            return c.chars().next().map(|c| c as i64).ok_or_else(|| "an empty character".into());
        }
        // (&x: a program's variable's address, which its name stands for
        // anyway, as C writes it for a struct.)
        if let Some(r) = t.strip_prefix('&') {
            *t = r;
            return unary(t, k);
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
        } else if let Some(r) = t.trim_start().strip_prefix('[') {
            // (A variable's element: `fds[1]`, its value, `known`'s as
            // "fds[1]".)
            *t = r;
            let i = or(t, k)?;
            *t = t.trim_start().strip_prefix(']').ok_or("a ] missing")?;
            k(&format!("{word}[{i}]")).ok_or_else(|| format!("no {word}[{i}]"))
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

/// A program's line (see statement): a call, `[runner:] [type] [name =]
/// syscall(args)` (its arguments as written: Types::arg reads each), or a
/// variable's declaration, `[runner:] type name[n] = init` (Types::declare
/// reads it).
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Call { tid: Option<String>, ty: Option<String>, name: Option<String>, call: String, args: Vec<String> },
    Decl { tid: Option<String>, text: String },
}

/// A program's line, parsed (see Statement): a call if it ends in one
/// (`name(…)`, its parentheses the line's end), else a declaration.
pub fn statement(text: &str) -> Result<Statement, String> {
    let s = text.trim().trim_end_matches(';').trim();
    let word = |w: &str| !w.is_empty() && w.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$') && !w.starts_with(|c: char| c.is_ascii_digit());
    // (`19:` or `child:`, its runner; not `::`.)
    let (tid, s) = match s.split_once(':') {
        Some((t, r)) if word(t.trim()) || t.trim().chars().all(|c| c.is_ascii_digit()) && !t.trim().is_empty() => (Some(t.trim().to_string()), r.trim()),
        _ => (None, s),
    };
    // (The call: the first "(" outside strings, closed at the line's end.)
    let open = s.find('(');
    let close = open.and_then(|o| {
        let (mut depth, mut quote) = (0, false);
        let b = s.as_bytes();
        let mut i = o;
        while i < b.len() {
            match b[i] {
                b'\\' if quote => i += 1,
                b'"' => quote = !quote,
                b'(' | b'{' if !quote => depth += 1,
                b')' | b'}' if !quote => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
            i += 1;
        }
        None
    });
    let (Some(open), Some(close)) = (open, close) else { return Ok(Statement::Decl { tid, text: s.to_string() }) };
    if close + 1 != s.len() {
        return Ok(Statement::Decl { tid, text: s.to_string() });
    }
    let head = s[..open].trim();
    let (lhs, call) = match head.rsplit_once('=') {
        Some((l, c)) if !l.ends_with(['=', '!', '<', '>']) => (Some(l.trim()), c.trim()),
        _ => (None, head),
    };
    if !word(call) {
        return Err(format!("not a call: {head}"));
    }
    // (`int fd`, `void *mem`: the result's type, then its name.)
    let (ty, name) = match lhs {
        None => (None, None),
        Some(l) => {
            let at = l.rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).map_or(0, |k| k + 1);
            let (t, n) = (l[..at].trim(), &l[at..]);
            if !word(n) {
                return Err(format!("not a name for the result: {l}"));
            }
            (if t.is_empty() { None } else { Some(t.split_whitespace().collect::<Vec<_>>().join(" ")) }, Some(n.to_string()))
        }
    };
    let inner = &s[open + 1..close];
    let args = if inner.trim().is_empty() { Vec::new() } else { split(inner) };
    Ok(Statement::Call { tid, ty, name, call: call.to_string(), args })
}

/// `text` split at its commas outside strings, () and {}, each trimmed.
fn split(text: &str) -> Vec<String> {
    let (mut out, mut cur, mut depth, mut quote) = (Vec::new(), String::new(), 0, false);
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' if quote => {
                cur.push(c);
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
                continue;
            }
            '"' => quote = !quote,
            '(' | '{' if !quote => depth += 1,
            ')' | '}' if !quote => depth -= 1,
            ',' if !quote && depth == 0 => {
                out.push(core::mem::take(&mut cur).trim().to_string());
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    out.push(cur.trim().to_string());
    out
}

/// A value of the kernel's types as data (Types::value): a number, a
/// string (chars), a list (an array), the members of a struct.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Uint(u64),
    Float(f64),
    Text(String),
    List(Vec<Value>),
    Fields(Vec<(String, Value)>),
}

/// A C literal's item: a string, {…}, or an expression.
fn literal(v: &str) -> Result<Item, String> {
    Ok(match v.strip_prefix('{').and_then(|r| r.strip_suffix('}')) {
        Some(inner) => Item::List(items(inner)?),
        None if v.starts_with('"') => Item::Str(unquote(v)?),
        None => Item::Expr(v.to_string()),
    })
}

/// A program's variable, declared as in C (see Types::declare): its name,
/// its type (of each element, for an array of `count`), its size, and the
/// bytes it starts with.
#[derive(Debug, Clone, PartialEq)]
pub struct Decl {
    pub name: String,
    pub ty: u32,
    pub count: Option<u32>,
    pub bytes: Vec<u8>,
}

impl Types {
    /// A declaration, as in C, of a program's variable (memory of its own,
    /// in its process: its name then stands for its address): `char
    /// buf[64]`, `int fds[2]`, `struct __kernel_timespec ts = {0,
    /// 30000000}`, `char path[] = "/note.txt"`; the type the kernel's own
    /// (a struct, union or enum by its tag, a typedef, a base type by its
    /// name). `known`: as for arg.
    pub fn declare(&self, text: &str, known: &dyn Fn(&str) -> Option<i64>) -> Result<Decl, String> {
        let s = text.trim().trim_end_matches(';').trim();
        let (lhs, init) = match s.split_once('=') {
            Some((l, r)) => (l.trim(), Some(r.trim())),
            None => (s, None),
        };
        // (Its name, the last word, with [n] after it for an array.)
        let (head, count) = match lhs.strip_suffix(']').and_then(|l| l.rsplit_once('[')) {
            Some((h, c)) => (h.trim_end(), Some(c.trim())),
            None => (lhs, None),
        };
        let at = head.rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).ok_or("a declaration: a type, then a name")?;
        let (tyname, name) = (head[..=at].trim(), &head[at + 1..]);
        if tyname.is_empty() || name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
            return Err("a declaration: a type, then a name".into());
        }
        if tyname.ends_with('*') {
            return Err(format!("{name}: a pointer: declare what it points to (its name is its address)"));
        }
        let ty = self.named(tyname).ok_or_else(|| format!("no type {tyname} in the kernel"))?;
        let esize = self.size_of(ty) as usize;
        if esize == 0 {
            return Err(format!("{tyname}: no size"));
        }
        let item = init.map(literal).transpose()?;
        // (An array's count: as written, or, `[]`, as many as it starts with.)
        let count = match count {
            None => None,
            Some("") => Some(match &item {
                Some(Item::Str(s)) => s.len() as u32 + 1,
                Some(Item::List(l)) => l.len() as u32,
                _ => return Err(format!("{name}[]: how many?")),
            }),
            Some(c) => Some(u32::try_from(eval(c, known)?).map_err(|_| format!("{name}[{c}]: not a size"))?),
        };
        let bytes = self.lay(ty, count, item.as_ref(), known).map_err(|e| format!("{name}: {e}"))?;
        Ok(Decl { name: name.to_string(), ty, count, bytes })
    }

    /// A variable's bytes (of type `ty`, an array of `count` of them) as
    /// `text` makes them: a C literal ("hi", {3, 4}, {.tv_sec = 1}, 7).
    pub fn assign(&self, ty: u32, count: Option<u32>, text: &str, known: &dyn Fn(&str) -> Option<i64>) -> Result<Vec<u8>, String> {
        self.lay(ty, count, Some(&literal(text.trim())?), known)
    }

    /// The bytes of a variable of type `ty` (an array of `count`) that
    /// `item` makes (none: zeros).
    fn lay(&self, ty: u32, count: Option<u32>, item: Option<&Item>, known: &dyn Fn(&str) -> Option<i64>) -> Result<Vec<u8>, String> {
        let esize = self.size_of(ty) as usize;
        let mut bytes = vec![0u8; esize * count.unwrap_or(1) as usize];
        match (item, count) {
            (None, _) => {}
            (Some(Item::Str(s)), Some(n)) if self.is_string(ty) => {
                if s.len() + 1 > n as usize {
                    return Err(format!("\"{s}\": longer than {n} bytes"));
                }
                bytes[..s.len()].copy_from_slice(s.as_bytes());
            }
            (Some(Item::List(list)), Some(n)) => {
                if list.len() > n as usize {
                    return Err("too many values".into());
                }
                for (i, (_, it)) in list.iter().enumerate() {
                    self.fill(&mut bytes, i * esize, ty, it, known)?;
                }
            }
            (Some(it), None) => self.fill(&mut bytes, 0, ty, it, known)?,
            (Some(_), Some(_)) => return Err("an array is {…} (or a string, for chars)".into()),
        }
        Ok(bytes)
    }

    /// A value of type `ty` at `addr` (an array of `count`) as data: what a
    /// JavaScript program reads it as.
    pub fn value(&self, mem: &dyn Memory, addr: u32, ty: u32, count: Option<u32>) -> Value {
        let esize = self.size_of(ty);
        if let Some(n) = count {
            if self.is_string(ty) {
                let mut b = vec![0u8; n.min(4096) as usize];
                mem.read(addr, &mut b);
                let len = b.iter().position(|&c| c == 0).unwrap_or(b.len());
                return Value::Text(String::from_utf8_lossy(&b[..len]).into_owned());
            }
            return Value::List((0..n.min(4096)).map(|i| self.value(mem, addr + i * esize, ty, None)).collect());
        }
        let id = self.resolve(ty);
        match &self.get(id).kind {
            k @ (Kind::Struct(_) | Kind::Union(_)) => Value::Fields(self.members(k).iter().filter(|m| !m.name.is_empty()).map(|m| {
                let v = if m.bit_size > 0 {
                    Value::Int(self.read_int(mem, addr + m.bit_offset / 8, 0, m.bit_offset % 8, m.bit_size) as i64)
                } else {
                    self.value(mem, addr + m.bit_offset / 8, m.ty, None)
                };
                (m.name.clone(), v)
            }).collect()),
            Kind::Array { elem, count } => self.value(mem, addr, *elem, Some(*count)),
            Kind::Pointer(_) => Value::Int(mem.read_u32(addr) as i64),
            Kind::Base(Base::Float) if esize == 4 => Value::Float(f32::from_bits(mem.read_u32(addr)) as f64),
            Kind::Base(Base::Float) => Value::Float(f64::from_bits(self.read_int(mem, addr, 8, 0, 0))),
            Kind::Base(Base::Signed | Base::Char) | Kind::Enum(_) => {
                let bits = esize.clamp(1, 8) * 8;
                Value::Int(((self.read_int(mem, addr, esize.min(8), 0, 0) << (64 - bits)) as i64) >> (64 - bits))
            }
            Kind::Base(_) => Value::Uint(self.read_int(mem, addr, esize.min(8), 0, 0)),
            _ => Value::Int(0),
        }
    }

    /// The type named `name` as C writes it: "struct timespec", a typedef
    /// ("loff_t"), a base type ("unsigned int").
    pub fn named(&self, name: &str) -> Option<u32> {
        let tag = ["struct ", "union ", "enum "].iter().find_map(|p| name.strip_prefix(p));
        match tag {
            Some(t) => self.find(t.trim()),
            None => self.find(name).or_else(|| self.types.iter().position(|t| t.name == name && matches!(t.kind, Kind::Base(_))).map(|k| k as u32)),
        }
    }

    /// The `len` bytes at `addr` as records of struct `ty`, if it is one of
    /// variable length (it ends in a flexible array, and has a member that
    /// says each record's length: arch::RECORD_LENGTHS), one after another:
    /// "[{d_ino = 1, …, d_name = \".\"}, …]", as the kernel's type reads them.
    pub fn records(&self, mem: &dyn Memory, addr: u32, len: u32, ty: u32) -> Option<String> {
        let members = self.members(&self.get(self.resolve(ty)).kind);
        let last = members.last()?;
        let Kind::Array { elem, count: 0 } = self.get(self.resolve(last.ty)).kind else { return None };
        let size = members.iter().find(|m| crate::arch::RECORD_LENGTHS.contains(&m.name.as_str()))?;
        let mut out = Vec::new();
        let mut at = 0;
        while at < len && out.len() < 64 {
            let rec = addr + at;
            let reclen = self.read_int(mem, rec + size.bit_offset / 8, self.size_of(size.ty).min(8), 0, 0) as u32;
            if reclen == 0 {
                break;
            }
            let fields: Vec<String> = members.iter().filter(|m| m.bit_size == 0).map(|m| {
                let off = m.bit_offset / 8;
                let v = if core::ptr::eq(m, last) {
                    self.show(mem, rec + off, elem, Some(reclen.saturating_sub(off)))
                } else {
                    let mut v = String::new();
                    self.print(mem, rec + off, m.ty, 0, 0, &mut v);
                    v
                };
                format!("{} = {v}", m.name)
            }).collect();
            out.push(format!("{{{}}}", fields.join(", ")));
            at += reclen;
        }
        Some(format!("[{}]", out.join(", ")))
    }

    /// A value of type `ty` at `addr` (an array of `count` of them), said
    /// as C would write it: `"hello"` for chars, `{3, 4}`, `{tv_sec = 0,
    /// tv_nsec = 30000000}`.
    pub fn show(&self, mem: &dyn Memory, addr: u32, ty: u32, count: Option<u32>) -> String {
        let esize = self.size_of(ty);
        match count {
            // (Chars: a C string, to its last byte not 0: text as text, and
            // what is not, a NUL or a byte of a record, escaped.)
            Some(n) if self.is_string(ty) => {
                let mut b = vec![0u8; n.min(4096) as usize];
                mem.read(addr, &mut b);
                let len = b.iter().rposition(|&c| c != 0).map_or(0, |k| k + 1);
                let mut s = String::from("\"");
                for (k, &c) in b[..len].iter().enumerate() {
                    if k == 64 {
                        s.push_str("\"…");
                        return s;
                    }
                    match c {
                        b'\n' => s.push_str("\\n"),
                        b'\t' => s.push_str("\\t"),
                        b'"' => s.push_str("\\\""),
                        b'\\' => s.push_str("\\\\"),
                        0 => s.push_str("\\0"),
                        0x20..0x7f => s.push(c as char),
                        _ => s.push_str(&format!("\\x{c:02x}")),
                    }
                }
                s.push('"');
                s
            }
            Some(n) => format!("{{{}}}", (0..n.min(16)).map(|i| self.show(mem, addr + i * esize, ty, None)).collect::<Vec<_>>().join(", ")),
            None => match &self.get(self.resolve(ty)).kind {
                k @ (Kind::Struct(_) | Kind::Union(_)) => format!("{{{}}}", self.members(k).iter().filter(|m| m.bit_size == 0)
                    .map(|m| {
                        let mut v = String::new();
                        self.print(mem, addr + m.bit_offset / 8, m.ty, 0, 0, &mut v);
                        format!("{} = {v}", m.name)
                    }).collect::<Vec<_>>().join(", ")),
                _ => {
                    let mut v = String::new();
                    self.print(mem, addr, ty, 0, 0, &mut v);
                    v
                }
            },
        }
    }

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
    fn a_line_is_a_call_or_a_declaration() {
        let call = |t: &str| match statement(t).unwrap() {
            Statement::Call { tid, ty, name, call, args } => (tid, ty, name, call, args),
            s => panic!("{s:?}"),
        };
        let o = |s: &str| Some(s.to_string());
        assert_eq!(call("int fd = openat(AT_FDCWD, \"/a, b\", O_RDONLY);"),
                   (None, o("int"), o("fd"), "openat".into(), alloc::vec!["AT_FDCWD".into(), "\"/a, b\"".into(), "O_RDONLY".into()]));
        assert_eq!(call("child: void *mem = mmap2(NULL, {1, 2}, 3)"), (o("child"), o("void *"), o("mem"), "mmap2".into(), alloc::vec!["NULL".into(), "{1, 2}".into(), "3".into()]));
        assert_eq!(call("fd = getpid()"), (None, None, o("fd"), "getpid".into(), alloc::vec![]));
        assert_eq!(statement("char buf[64]").unwrap(), Statement::Decl { tid: None, text: "char buf[64]".into() });
        assert_eq!(statement("19: struct __kernel_timespec ts = {0, 1}").unwrap(), Statement::Decl { tid: o("19"), text: "struct __kernel_timespec ts = {0, 1}".into() });
    }

    #[test]
    fn declarations_are_a_programs_memory() {
        let t = Types::parse(TYPES);
        let d = t.declare("struct timespec ts = {0, 30000000}", &known).unwrap();
        assert_eq!((d.name.as_str(), d.ty, d.count, d.bytes.len()), ("ts", 2, None, 16));
        assert_eq!(&d.bytes[8..12], &30000000u32.to_le_bytes());
        let buf = t.declare("char buf[64]", &known).unwrap();
        assert_eq!((buf.ty, buf.count, buf.bytes.len()), (4, Some(64), 64));
        let fds = t.declare("int fds[2] = {3, 4}", &known).unwrap();
        assert_eq!(fds.bytes, [3, 0, 0, 0, 4, 0, 0, 0]);
        assert_eq!(t.declare("char path[] = \"/x\"", &known).unwrap().bytes, b"/x\0");
        assert!(t.declare("struct nope x", &known).is_err() && t.declare("char *p", &known).is_err());
        // Said as C writes it.
        struct M(Vec<u8>);
        impl Memory for M {
            fn read(&self, addr: u32, buf: &mut [u8]) {
                buf.copy_from_slice(&self.0[addr as usize..addr as usize + buf.len()]);
            }
        }
        let mem = M([fds.bytes.clone(), d.bytes.clone(), b"hi\0".to_vec()].concat());
        assert_eq!(t.show(&mem, 0, 7, Some(2)), "{3, 4}");
        assert_eq!(t.show(&mem, 8, 2, None), "{tv_sec = 0, tv_nsec = 30000000}");
        assert_eq!(t.show(&mem, 24, 4, Some(3)), "\"hi\"");
        // As data (for JavaScript), and assigned from a literal.
        assert_eq!(t.value(&mem, 0, 7, Some(2)), Value::List(alloc::vec![Value::Int(3), Value::Int(4)]));
        assert_eq!(t.value(&mem, 8, 2, None), Value::Fields(alloc::vec![("tv_sec".into(), Value::Int(0)), ("tv_nsec".into(), Value::Int(30000000))]));
        assert_eq!(t.value(&mem, 24, 4, Some(3)), Value::Text("hi".into()));
        assert_eq!(t.assign(7, Some(2), "{5, 6}", &known).unwrap(), [5, 0, 0, 0, 6, 0, 0, 0]);
        // (Past a NUL too, escaped, as C writes it.)
        assert_eq!(t.show(&M(b"a\0b\x05\0\0".to_vec()), 0, 4, Some(6)), "\"a\\0b\\x05\"");
    }

    #[test]
    fn expressions() {
        assert_eq!(eval("O_CREAT | O_WRONLY", &known), Ok(65));
        assert_eq!(eval("0644", &known), Ok(420));
        assert_eq!(eval("(1 << 4) + fd - $2", &known), Ok(14));
        assert_eq!((eval("'h'", &known), eval("'\\n'", &known), eval("'\\0' + 1", &known)), (Ok(104), Ok(10), Ok(1)));
        assert_eq!(eval("fds[1] + &fd", &|n| [("fds[1]", 4), ("fd", 3)].iter().find(|(k, _)| *k == n).map(|(_, v)| *v)), Ok(7));
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
