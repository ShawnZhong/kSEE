//! C expressions, as a program's lines write them: C's operators and
//! precedence, over names that are typed (see Sym): a variable is memory of
//! its type, a result a value of its declared type. `.`, `->`, `[]`, `*`,
//! `&`, casts, `sizeof` and pointer arithmetic mean what they mean in C,
//! reading the program's memory (Env::read) and the kernel's types.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::types::{Base, Kind, Types};
use crate::Memory;

/// What a name stands for.
#[derive(Debug, Clone, PartialEq)]
pub enum Sym {
    /// A number (a constant, a call's result); `to`: if it is a pointer,
    /// the type it points to (0: void).
    Value { v: i64, to: Option<u32> },
    /// A variable: memory of type `ty` at `addr` (an array of `count`).
    Var { addr: u32, ty: u32, count: Option<u32> },
}

/// The names an expression may use, and the memory it reads.
pub trait Env {
    fn sym(&self, name: &str) -> Option<Sym>;
    fn memory(&self) -> Option<&dyn Memory> {
        None
    }
}

/// Numbers by name only (the kernel's constants as they are defined, in
/// ksee pack; a test's).
impl<F: Fn(&str) -> Option<i64> + ?Sized> Env for F {
    fn sym(&self, name: &str) -> Option<Sym> {
        self(name).map(|v| Sym::Value { v, to: None })
    }
}

/// A value: a number, a pointer (to type `to`, 0: void), or memory (an
/// lvalue: at `addr`, of type `ty`, an array of `count`).
#[derive(Debug, Clone, Copy, PartialEq)]
enum V {
    Int(i64),
    Ptr { addr: u32, to: u32 },
    Lv { addr: u32, ty: u32, count: Option<u32> },
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(i64),
    Name(String),
    Op(&'static str),
}

const OPS: [&str; 29] = ["<<", ">>", "<=", ">=", "==", "!=", "&&", "||", "->", "(", ")", "[", "]", ".", ",", "*", "/", "%", "+", "-",
                         "~", "!", "&", "|", "^", "<", ">", "?", ":"];

fn tokens(s: &str) -> Result<Vec<Tok>, String> {
    let mut out = Vec::new();
    let mut t = s;
    loop {
        t = t.trim_start();
        let Some(c) = t.chars().next() else { return Ok(out) };
        if c.is_ascii_digit() {
            let n = t.find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(t.len());
            let (word, rest) = t.split_at(n);
            let num = word.trim_end_matches(['U', 'L', 'u', 'l']);
            let v = if let Some(h) = num.strip_prefix("0x").or_else(|| num.strip_prefix("0X")) {
                i64::from_str_radix(h, 16)
            } else if num.len() > 1 && num.starts_with('0') {
                i64::from_str_radix(&num[1..], 8)
            } else {
                num.parse()
            };
            out.push(Tok::Num(v.map_err(|_| format!("not a number: {word}"))?));
            t = rest;
        } else if c.is_ascii_alphabetic() || c == '_' || c == '$' {
            let n = t.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$')).unwrap_or(t.len());
            out.push(Tok::Name(t[..n].to_string()));
            t = &t[n..];
        } else if c == '\'' {
            // ('h', '\n': a character's number: its byte, or its first UTF-8 byte.)
            let r = &t[1..];
            let end = r.char_indices().skip(1).find(|&(i, c)| c == '\'' && !r[..i].ends_with('\\')).map(|(i, _)| i)
                .or_else(|| r.find('\'')).ok_or("a ' missing")?;
            let b = super::text::unquote(&format!("\"{}\"", &r[..end]))?;
            out.push(Tok::Num(*b.first().ok_or("an empty character")? as i64));
            t = &r[end + 1..];
        } else {
            let op = OPS.iter().find(|o| t.starts_with(**o)).ok_or_else(|| format!("not an argument: {t}"))?;
            out.push(Tok::Op(op));
            t = &t[op.len()..];
        }
    }
}

/// The words that begin a type name in a cast or sizeof.
const TYPE_WORDS: [&str; 13] = ["struct", "union", "enum", "unsigned", "signed", "char", "short", "int", "long", "void", "const", "_Bool", "volatile"];

struct P<'a> {
    toks: Vec<Tok>,
    at: usize,
    types: Option<&'a Types>,
    env: &'a dyn Env,
}

impl P<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at)
    }
    fn eat(&mut self, op: &str) -> bool {
        let yes = matches!(self.peek(), Some(Tok::Op(o)) if *o == op);
        self.at += yes as usize;
        yes
    }
    fn expect(&mut self, op: &str) -> Result<(), String> {
        if self.eat(op) { Ok(()) } else { Err(format!("a {op} missing")) }
    }
    fn types(&self) -> Result<&Types, String> {
        self.types.ok_or_else(|| "types: none here".into())
    }
    fn mem(&self) -> Result<&dyn Memory, String> {
        self.env.memory().ok_or_else(|| "memory: none here".into())
    }

    /// A type name, at a cast's or sizeof's "(": its type, and how many
    /// pointers deep (`struct io_uring_sqe *`: that struct, 1), if it is one.
    fn type_name(&mut self) -> Result<Option<(u32, u32)>, String> {
        let start = self.at;
        let mut words = Vec::new();
        while let Some(Tok::Name(w)) = self.peek() {
            words.push(w.clone());
            self.at += 1;
        }
        let first = words.first().map(String::as_str);
        let known = self.types.and_then(|t| first.and_then(|f| t.named(f)));
        if words.is_empty() || !(TYPE_WORDS.contains(&first.unwrap_or("")) || known.is_some() && matches!(self.peek(), Some(Tok::Op("*" | ")")))) {
            self.at = start;
            return Ok(None);
        }
        let mut depth = 0;
        while self.eat("*") {
            depth += 1;
        }
        let words: Vec<&str> = words.iter().map(String::as_str).filter(|w| !matches!(*w, "const" | "volatile")).collect();
        let name = words.join(" ");
        let base = match name.as_str() {
            "void" => 0,
            "unsigned" => self.types()?.named("unsigned int").ok_or("no type unsigned int")?,
            "signed" => self.types()?.named("int").ok_or("no type int")?,
            n => self.types()?.named(n).ok_or_else(|| format!("no type {n} in the kernel"))?,
        };
        Ok(Some((base, depth)))
    }

    fn expr(&mut self) -> Result<V, String> {
        self.binary(0)
    }

    /// Binary operators, by C's precedence (loosest first).
    fn binary(&mut self, level: usize) -> Result<V, String> {
        const LEVELS: [&[&str]; 10] = [&["||"], &["&&"], &["|"], &["^"], &["&"], &["==", "!="], &["<", ">", "<=", ">="], &["<<", ">>"], &["+", "-"], &["*", "/", "%"]];
        if level == LEVELS.len() {
            return self.unary();
        }
        let mut l = self.binary(level + 1)?;
        while let Some(&Tok::Op(op)) = self.peek() {
            if !LEVELS[level].contains(&op) {
                break;
            }
            self.at += 1;
            let r = self.binary(level + 1)?;
            l = self.apply(op, l, r)?;
        }
        Ok(l)
    }

    fn apply(&self, op: &str, l: V, r: V) -> Result<V, String> {
        let (l, r) = (self.rvalue(l)?, self.rvalue(r)?);
        // (Pointer arithmetic: by the size of what it points to; void's: 1.)
        let size = |to: u32| if to == 0 { Ok(1) } else { self.types().map(|t| t.size_of(to).max(1) as i64) };
        Ok(match (op, l, r) {
            ("+", V::Ptr { addr, to }, V::Int(i)) | ("+", V::Int(i), V::Ptr { addr, to }) => V::Ptr { addr: (addr as i64 + i * size(to)?) as u32, to },
            ("-", V::Ptr { addr, to }, V::Int(i)) => V::Ptr { addr: (addr as i64 - i * size(to)?) as u32, to },
            ("-", V::Ptr { addr: a, to }, V::Ptr { addr: b, .. }) => V::Int((a as i64 - b as i64) / size(to)?),
            _ => {
                let (a, b) = (num(l), num(r));
                V::Int(match op {
                    "||" => (a != 0 || b != 0) as i64,
                    "&&" => (a != 0 && b != 0) as i64,
                    "|" => a | b,
                    "^" => a ^ b,
                    "&" => a & b,
                    "==" => (a == b) as i64,
                    "!=" => (a != b) as i64,
                    "<" => (a < b) as i64,
                    ">" => (a > b) as i64,
                    "<=" => (a <= b) as i64,
                    ">=" => (a >= b) as i64,
                    "<<" => a.checked_shl(b as u32).ok_or("a shift too far")?,
                    ">>" => a.checked_shr(b as u32).ok_or("a shift too far")?,
                    "+" => a.wrapping_add(b),
                    "-" => a.wrapping_sub(b),
                    "*" => a.wrapping_mul(b),
                    "/" => a.checked_div(b).ok_or("a division by 0")?,
                    "%" => a.checked_rem(b).ok_or("a division by 0")?,
                    _ => return Err(format!("not an operator here: {op}")),
                })
            }
        })
    }

    fn unary(&mut self) -> Result<V, String> {
        for op in ["-", "~", "!", "*", "&", "+"] {
            if self.eat(op) {
                let v = self.unary()?;
                return match op {
                    "&" => match v {
                        V::Lv { addr, ty, .. } => Ok(V::Ptr { addr, to: ty }),
                        _ => Err("&: of a variable, or a member of one".into()),
                    },
                    "*" => match self.rvalue(v)? {
                        V::Ptr { to: 0, .. } => Err("*: of a void *: cast it first".into()),
                        V::Ptr { addr, to } => Ok(self.place(addr, to)),
                        V::Int(_) => Err("*: of a number, not a pointer: cast it first".into()),
                        V::Lv { .. } => unreachable!(),
                    },
                    _ => {
                        let n = num(self.rvalue(v)?);
                        Ok(V::Int(match op { "-" => n.wrapping_neg(), "~" => !n, "!" => (n == 0) as i64, _ => n }))
                    }
                };
            }
        }
        if matches!(self.peek(), Some(Tok::Name(n)) if n == "sizeof") {
            self.at += 1;
            self.expect("(")?;
            let size = match self.type_name()? {
                Some((_, d)) if d > 0 => 4,
                Some((ty, _)) => self.types()?.size_of(ty),
                None => match self.expr()? {
                    V::Lv { ty, count, .. } => self.types()?.size_of(ty) * count.unwrap_or(1),
                    _ => 4,
                },
            };
            self.expect(")")?;
            return Ok(V::Int(size as i64));
        }
        // (A cast: a type name in parentheses.)
        if matches!(self.peek(), Some(Tok::Op("("))) {
            let back = self.at;
            self.at += 1;
            if let Some((ty, depth)) = self.type_name()? {
                self.expect(")")?;
                let v = self.unary()?;
                let v = self.rvalue(v)?;
                return Ok(match depth {
                    0 => V::Int(self.convert(num(v), ty)?),
                    1 => V::Ptr { addr: num(v) as u32, to: ty },
                    _ => V::Ptr { addr: num(v) as u32, to: self.pointer_to(ty, depth - 1)? },
                });
            }
            self.at = back;
        }
        self.postfix()
    }

    fn postfix(&mut self) -> Result<V, String> {
        let mut v = self.primary()?;
        loop {
            if self.eat("[") {
                let i = self.expr()?;
                let i = num(self.rvalue(i)?);
                self.expect("]")?;
                v = match self.rvalue(v)? {
                    V::Ptr { to: 0, .. } => return Err("[]: of a void *: cast it first".into()),
                    V::Ptr { addr, to } => self.place((addr as i64 + i * self.types()?.size_of(to) as i64) as u32, to),
                    _ => return Err("[]: of a number, not an array or a pointer".into()),
                };
            } else if self.eat(".") || matches!(self.peek(), Some(Tok::Op("->"))) {
                let arrow = self.eat("->");
                let Some(Tok::Name(m)) = self.peek().cloned() else { return Err("a member's name missing".into()) };
                self.at += 1;
                let (addr, ty) = match (arrow, if arrow { self.rvalue(v)? } else { v }) {
                    (false, V::Lv { addr, ty, count: None }) => (addr, ty),
                    (true, V::Ptr { addr, to }) if to != 0 => (addr, to),
                    _ => return Err(format!("{}{m}: not a struct{}", if arrow { "->" } else { "." }, if arrow { " pointer" } else { "" })),
                };
                let t = self.types()?;
                let member = t.members(&t.get(t.resolve(ty)).kind).into_iter().find(|x| x.name == m)
                    .ok_or_else(|| format!("{} has no member {m}", t.name(ty)))?;
                if member.bit_size > 0 {
                    return Err(format!("{m}: a bitfield"));
                }
                v = self.place(addr + member.bit_offset / 8, member.ty);
            } else {
                return Ok(v);
            }
        }
    }

    fn primary(&mut self) -> Result<V, String> {
        match self.peek().cloned() {
            Some(Tok::Num(n)) => {
                self.at += 1;
                Ok(V::Int(n))
            }
            Some(Tok::Name(n)) => {
                self.at += 1;
                if n == "NULL" {
                    return Ok(V::Ptr { addr: 0, to: 0 });
                }
                match self.env.sym(&n) {
                    Some(Sym::Value { v, to: None }) => Ok(V::Int(v)),
                    Some(Sym::Value { v, to: Some(to) }) => Ok(V::Ptr { addr: v as u32, to }),
                    Some(Sym::Var { addr, ty, count }) => Ok(V::Lv { addr, ty, count }),
                    None => Err(format!("not a number, a constant or a name of the program's: {n}")),
                }
            }
            Some(Tok::Op("(")) => {
                self.at += 1;
                let v = self.expr()?;
                self.expect(")")?;
                Ok(v)
            }
            Some(Tok::Op(o)) => Err(format!("not an argument: {o}")),
            None => Err("a value missing".into()),
        }
    }

    /// Memory of type `ty` at `addr`: an array's with its count.
    fn place(&self, addr: u32, ty: u32) -> V {
        match self.types.map(|t| (t, t.get(t.resolve(ty)).kind.clone())) {
            Some((t, Kind::Array { elem, count })) => V::Lv { addr, ty: t.resolve(elem), count: Some(count) },
            _ => V::Lv { addr, ty, count: None },
        }
    }

    /// A value as C reads it: memory's, by its type (an array: its first
    /// element's address).
    fn rvalue(&self, v: V) -> Result<V, String> {
        let V::Lv { addr, ty, count } = v else { return Ok(v) };
        if count.is_some() {
            return Ok(V::Ptr { addr, to: ty });
        }
        let t = self.types()?;
        let id = t.resolve(ty);
        match t.get(id).kind {
            Kind::Pointer(to) => Ok(V::Ptr { addr: self.mem()?.read_u32(addr), to: t.resolve(to) }),
            Kind::Base(_) | Kind::Enum(_) => {
                let size = t.size_of(id).clamp(1, 8) as usize;
                let mut b = [0u8; 8];
                self.mem()?.read(addr, &mut b[..size]);
                Ok(V::Int(self.convert(i64::from_le_bytes(b), id)?))
            }
            _ => Err(format!("{}: a struct: take its address (&), or a member", t.name(ty))),
        }
    }

    /// `v` as a value of integer type `ty`: its size, signed or not.
    fn convert(&self, v: i64, ty: u32) -> Result<i64, String> {
        let t = self.types()?;
        let bits = t.size_of(ty).clamp(1, 8) * 8;
        if bits == 64 {
            return Ok(v);
        }
        let signed = matches!(t.get(t.resolve(ty)).kind, Kind::Base(Base::Signed) | Kind::Enum(_));
        let masked = v & ((1i64 << bits) - 1);
        Ok(if signed && masked >> (bits - 1) & 1 == 1 { masked - (1i64 << bits) } else { masked })
    }

    /// A pointer type to `ty`, `depth` deep, as the kernel's types have it.
    fn pointer_to(&self, mut ty: u32, depth: u32) -> Result<u32, String> {
        let t = self.types()?;
        for _ in 0..depth {
            ty = (0..t.types.len() as u32).find(|&k| matches!(t.get(k).kind, Kind::Pointer(to) if t.resolve(to) == ty))
                .ok_or_else(|| format!("no pointer to {} in the kernel", t.name(ty)))?;
        }
        Ok(ty)
    }
}

fn num(v: V) -> i64 {
    match v {
        V::Int(n) => n,
        V::Ptr { addr, .. } => addr as i64,
        V::Lv { addr, .. } => addr as i64,
    }
}

fn parse<'a>(s: &str, types: Option<&'a Types>, env: &'a dyn Env) -> Result<(P<'a>, V), String> {
    let mut p = P { toks: tokens(s)?, at: 0, types, env };
    let v = p.expr()?;
    if p.at < p.toks.len() {
        return Err(format!("not an argument: {}", s.trim()));
    }
    Ok((p, v))
}

/// Expression `s`'s value (a pointer's: its address), with `types` for
/// casts, members and memory (none: numbers and names only).
pub fn value(s: &str, types: Option<&Types>, env: &dyn Env) -> Result<i64, String> {
    let (p, v) = parse(s, types, env)?;
    Ok(num(p.rvalue(v)?))
}

/// Expression `s` as memory to write (an lvalue): its address and type.
pub fn lvalue(s: &str, types: &Types, env: &dyn Env) -> Result<(u32, u32), String> {
    match parse(s, Some(types), env)?.1 {
        V::Lv { addr, ty, count: None } => Ok((addr, ty)),
        V::Lv { .. } => Err(format!("{}: an array: one of its elements", s.trim())),
        _ => Err(format!("{}: not a variable, a member of one, or what a pointer points to", s.trim())),
    }
}

/// A result's value as its declared type (`void *`, `struct x *`, `int`)
/// makes it: a number, or a pointer to what the type names.
pub fn typed(v: i64, ty: &str, types: &Types) -> Option<Sym> {
    let depth = ty.chars().filter(|&c| c == '*').count() as u32;
    if depth == 0 {
        return Some(Sym::Value { v, to: None });
    }
    let env = |_: &str| None;
    let mut p = P { toks: tokens(ty).ok()?, at: 0, types: Some(types), env: &env };
    let (base, d) = p.type_name().ok()??;
    let to = if d <= 1 { base } else { p.pointer_to(base, d - 1).ok()? };
    Some(Sym::Value { v, to: Some(to) })
}
