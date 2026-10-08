//! Values laid out in the kernel's types (a syscall's argument, a
//! program's variable), and read back as data or as C writes them.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::{format, vec};

use super::eval::{Env, Eval};
use super::parse::{Expr, Statement};
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
    /// in its process): `char buf[64]`, `int fds[2]`, `struct
    /// __kernel_timespec ts = {0, 30000000}`, `char path[] = "/note.txt"`,
    /// the type the
    /// kernel's own (a struct, union or enum by its tag, a typedef, a base
    /// type by its name). `known`: as for arg.
    pub(crate) fn declare(&self, line: &Statement, known: &dyn Env) -> Result<Decl, String> {
        let Statement::Decl { ty, name, count, init } = line else {
            return Err("a declaration: a type, then a name".into());
        };
        let (e, name) = (Eval { types: Some(self), env: known }, name.clone());
        let ty = match e.type_of(ty)? {
            (_, Some(_)) => return Err(format!("{name}: a pointer: declare what it points to (its name is its address)")),
            (0, None) => return Err(format!("{name}: void")),
            (t, None) => t,
        };
        if self.size_of(ty) == 0 {
            return Err(format!("{name}: no size"));
        }
        // (An array's count: as written, or, `[]`, as many as it starts with.)
        let count = match count {
            None => None,
            Some(None) => Some(match &init {
                Some(Expr::Str(s)) => s.len() as u32 + 1,
                Some(Expr::Init(l)) => l.len() as u32,
                _ => return Err(format!("{name}[]: how many?")),
            }),
            Some(Some(c)) => Some(u32::try_from(e.number(c)?).map_err(|_| format!("{name}[…]: not a size"))?),
        };
        let bytes = self.lay(ty, count, init.as_ref(), known).map_err(|e| format!("{name}: {e}"))?;
        Ok(Decl { name, ty, count, bytes })
    }

    /// The bytes of a variable of type `ty` (an array of `count`) that
    /// `init` makes (none: zeros).
    fn lay(&self, ty: u32, count: Option<u32>, init: Option<&Expr>, known: &dyn Env) -> Result<Vec<u8>, String> {
        let mut bytes = vec![0u8; self.size_of(ty) as usize * count.unwrap_or(1) as usize];
        match (init, count) {
            (None, _) => {}
            (Some(it), Some(n)) => self.fill_array(&mut bytes, 0, ty, n, it, known)?,
            (Some(it), None) => self.fill(&mut bytes, 0, ty, it, known)?,
        }
        Ok(bytes)
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
                    self.print(mem, rec + off, m.ty, 0, 0)
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
                k @ (Kind::Struct(_) | Kind::Union(_)) => {
                    // (Short: no zero or empty scalars, no nested aggregates
                    // (openable in the tree), at most 4 fields, then …; an
                    // all-zero struct is {0}. A union's members alias the
                    // same bytes: only its first.)
                    let union = matches!(k, Kind::Union(_));
                    // (At most this many fields, then ….)
                    const SUMMARY_FIELDS: usize = 4;
                    let mut fields: Vec<String> = Vec::new();
                    for m in self.members(k).iter().filter(|m| m.bit_size == 0).take(if union { 1 } else { usize::MAX }) {
                        let v = self.print(mem, addr + m.bit_offset / 8, m.ty, 0, 0);
                        if v.starts_with('{') || matches!(v.as_str(), "0" | "0x0" | "0.0" | "NULL" | "\"\"") {
                            continue;
                        }
                        if fields.len() == SUMMARY_FIELDS {
                            fields.push("…".into());
                            break;
                        }
                        fields.push(format!("{} = {v}", m.name));
                    }
                    if fields.is_empty() {
                        "{0}".into()
                    } else {
                        format!("{{{}}}", fields.join(", "))
                    }
                }
                _ => self.print(mem, addr, ty, 0, 0),
            },
        }
    }

    /// Argument `text` for parameter type `ty` (a syscall's, or 0: unknown):
    /// see the module's doc. `known`: the values of names (constants,
    /// results, variables).
    pub fn arg(&self, e: &Expr, ty: u32, known: &dyn Env) -> Result<Arg, String> {
        match e {
            Expr::Str(s) => {
                let mut b = s.clone();
                b.push(0);
                Ok(Arg::Bytes(b))
            }
            Expr::Init(list) => {
                // (The parameter's type: what the braces make.)
                let to = match self.get(self.resolve(ty)).kind {
                    Kind::Pointer(to) => self.resolve(to),
                    _ if ty == 0 => return Err("{…}: a parameter of no known type".into()),
                    _ => return Err(format!("{{…}} for a {}: not a pointer", self.name(ty))),
                };
                // A char **: an array of strings.
                if matches!(self.get(to).kind, Kind::Pointer(c) if self.is_string(c)) {
                    let ev = Eval { types: Some(self), env: known };
                    return list.iter().map(|it| match it {
                        Expr::Str(s) => Ok(Some(String::from_utf8_lossy(s).into_owned())),
                        it if ev.number(it)? == 0 => Ok(None),
                        _ => Err("an array of strings: \"…\" or NULL each".to_string()),
                    }).collect::<Result<_, _>>().map(Arg::Strings);
                }
                let mut b = vec![0u8; self.size_of(to) as usize];
                self.fill(&mut b, 0, to, e, known)?;
                Ok(Arg::Bytes(b))
            }
            _ => Ok(Arg::Number(Eval { types: Some(self), env: known }.number(e)?)),
        }
    }

    /// Put `init` into `buf` at `at`, as a value of type `ty`.
    fn fill(&self, buf: &mut Vec<u8>, at: usize, ty: u32, init: &Expr, known: &dyn Env) -> Result<(), String> {
        let id = self.resolve(ty);
        let t = self.get(id);
        match (&t.kind, init) {
            (Kind::Struct(_) | Kind::Union(_), Expr::Init(list)) => {
                let members = self.members(&t.kind);
                for (k, it) in list.iter().enumerate() {
                    let m = members.get(k).ok_or_else(|| format!("{}: too many values", self.name(id)))?;
                    if m.bit_size > 0 {
                        return Err(format!("{}.{}: a bitfield", self.name(id), m.name));
                    }
                    self.fill(buf, at + (m.bit_offset / 8) as usize, m.ty, it, known)?;
                }
                Ok(())
            }
            (&Kind::Array { elem, count }, Expr::Str(_) | Expr::Init(_)) => self.fill_array(buf, at, elem, count, init, known),
            (_, Expr::Str(s)) => Err(format!("\"{}\" for a {}", String::from_utf8_lossy(s), self.name(id))),
            (_, Expr::Init(_)) => Err(format!("{{…}} for a {}", self.name(id))),
            (Kind::Base(Base::Float), _) => Err(format!("{}: a float", self.name(id))),
            (Kind::Base(_) | Kind::Enum(_) | Kind::Pointer(_), e) => {
                let v = Eval { types: Some(self), env: known }.number(e)?;
                let size = self.size_of(id).clamp(1, 8) as usize;
                buf[at..at + size].copy_from_slice(&v.to_le_bytes()[..size]);
                Ok(())
            }
            _ => Err(format!("a value for a {}", self.name(id))),
        }
    }

    /// Lay `init` out at `at` in `buf` as `count` of type `elem` (an
    /// array): a string, for chars; else {…}, an element each.
    fn fill_array(&self, buf: &mut Vec<u8>, at: usize, elem: u32, count: u32, init: &Expr, known: &dyn Env) -> Result<(), String> {
        let esize = self.size_of(elem) as usize;
        match init {
            Expr::Str(s) if self.is_string(elem) => {
                if s.len() + 1 > count as usize {
                    return Err(format!("\"{}\": longer than {count} bytes", String::from_utf8_lossy(s)));
                }
                buf[at..at + s.len()].copy_from_slice(s);
                Ok(())
            }
            Expr::Init(list) if list.len() > count as usize => Err(format!("too many values: {count} at most")),
            Expr::Init(list) => list.iter().enumerate().try_for_each(|(i, it)| self.fill(buf, at + i * esize, elem, it, known)),
            _ => Err("an array is {…} (or a string, for chars)".into()),
        }
    }
}
