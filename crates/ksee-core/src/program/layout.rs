//! Values laid out in the kernel's types (a syscall's argument, a
//! program's variable), and read back as data or as C writes them.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::{format, vec};

use super::expr::{value, Env};
use super::text::{items, literal, unquote, Item};
use crate::types::{Base, Kind, Types};
use crate::Memory;

/// An argument's value, as Machine::call takes it.
#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    Number(i64),
    /// A buffer with these bytes (a string: with its NUL).
    Bytes(Vec<u8>),
    /// An array of char * (None: NULL).
    Strings(Vec<Option<String>>),
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
    pub fn declare(&self, text: &str, known: &dyn Env) -> Result<Decl, String> {
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
            Some(c) => Some(u32::try_from(value(c, Some(self), known)?).map_err(|_| format!("{name}[{c}]: not a size"))?),
        };
        let bytes = self.lay(ty, count, item.as_ref(), known).map_err(|e| format!("{name}: {e}"))?;
        Ok(Decl { name: name.to_string(), ty, count, bytes })
    }

    /// A variable's bytes (of type `ty`, an array of `count` of them) as
    /// `text` makes them: a C literal ("hi", {3, 4}, {.tv_sec = 1}, 7).
    pub fn assign(&self, ty: u32, count: Option<u32>, text: &str, known: &dyn Env) -> Result<Vec<u8>, String> {
        self.lay(ty, count, Some(&literal(text.trim())?), known)
    }

    /// The bytes of a variable of type `ty` (an array of `count`) that
    /// `item` makes (none: zeros).
    fn lay(&self, ty: u32, count: Option<u32>, item: Option<&Item>, known: &dyn Env) -> Result<Vec<u8>, String> {
        let mut bytes = vec![0u8; self.size_of(ty) as usize * count.unwrap_or(1) as usize];
        match (item, count) {
            (None, _) => {}
            (Some(it), Some(n)) => self.fill_array(&mut bytes, 0, ty, n, it, known)?,
            (Some(it), None) => self.fill(&mut bytes, 0, ty, it, known)?,
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
                crate::types::quote(&b[..len])
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
    pub fn arg(&self, text: &str, ty: u32, known: &dyn Env) -> Result<Arg, String> {
        let s = text.trim();
        if s.starts_with('"') {
            let mut b = unquote(s)?;
            b.push(0);
            return Ok(Arg::Bytes(b));
        }
        if let Some(inner) = s.strip_prefix('{').and_then(|r| r.strip_suffix('}')) {
            let list = items(inner)?;
            // (The parameter's type: what the braces make.)
            let to = match self.get(self.resolve(ty)).kind {
                Kind::Pointer(to) => self.resolve(to),
                _ if ty == 0 => return Err("{…}: a parameter of no known type".into()),
                _ => return Err(format!("{{…}} for a {}: not a pointer", self.name(ty))),
            };
            // A char **: an array of strings.
            if matches!(self.get(to).kind, Kind::Pointer(c) if self.is_string(c)) {
                return list.into_iter().map(|(_, it)| match it {
                    Item::Str(s) => Ok(Some(String::from_utf8_lossy(&s).into_owned())),
                    Item::Expr(e) if value(&e, Some(self), known)? == 0 => Ok(None),
                    _ => Err("an array of strings: \"…\" or NULL each".to_string()),
                }).collect::<Result<_, _>>().map(Arg::Strings);
            }
            let mut b = vec![0u8; self.size_of(to) as usize];
            self.fill(&mut b, 0, to, &Item::List(list), known)?;
            return Ok(Arg::Bytes(b));
        }
        Ok(Arg::Number(value(s, Some(self), known)?))
    }

    /// Put `item` into `buf` at `at`, as a value of type `ty`.
    fn fill(&self, buf: &mut Vec<u8>, at: usize, ty: u32, item: &Item, known: &dyn Env) -> Result<(), String> {
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
            (&Kind::Array { elem, count }, Item::Str(_) | Item::List(_)) => self.fill_array(buf, at, elem, count, item, known),
            (Kind::Base(_) | Kind::Enum(_) | Kind::Pointer(_), Item::Expr(e)) => {
                let v = value(e, Some(self), known)?;
                let size = self.size_of(id).clamp(1, 8) as usize;
                if matches!(t.kind, Kind::Base(Base::Float)) {
                    return Err(format!("{}: a float", self.name(id)));
                }
                buf[at..at + size].copy_from_slice(&v.to_le_bytes()[..size]);
                Ok(())
            }
            (_, Item::Str(s)) => Err(format!("\"{}\" for a {}", String::from_utf8_lossy(s), self.name(id))),
            (_, Item::List(_)) => Err(format!("{{…}} for a {}", self.name(id))),
            (_, Item::Expr(e)) => Err(format!("{e} for a {}", self.name(id))),
        }
    }

    /// Lay `item` out at `at` in `buf` as `count` of type `elem` (an
    /// array): a string, for chars; else {…}, an element each.
    fn fill_array(&self, buf: &mut Vec<u8>, at: usize, elem: u32, count: u32, item: &Item, known: &dyn Env) -> Result<(), String> {
        let esize = self.size_of(elem) as usize;
        match item {
            Item::Str(s) if self.is_string(elem) => {
                if s.len() + 1 > count as usize {
                    return Err(format!("\"{}\": longer than {count} bytes", String::from_utf8_lossy(s)));
                }
                buf[at..at + s.len()].copy_from_slice(s);
                Ok(())
            }
            Item::List(list) if list.len() > count as usize => Err(format!("too many values: {count} at most")),
            Item::List(list) => list.iter().enumerate().try_for_each(|(i, (_, it))| self.fill(buf, at + i * esize, elem, it, known)),
            _ => Err("an array is {…} (or a string, for chars)".into()),
        }
    }
}
