//! A program's expressions evaluated as C does (see parse): over names
//! that are typed (Sym: a variable is memory of its type, a result a value
//! of its declared type), reading the program's memory and the kernel's
//! types: `[]`, `&`, and `+` on a pointer (not `.`, `->` or `*`: parse has
//! none).

use alloc::format;
use alloc::string::String;

use super::parse::{Expr, TypeName};
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
/// crates/ksee-cli's pack; a test's).
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

/// An evaluation: the kernel's types (none: numbers and names only), and
/// the program's names.
pub(super) struct Eval<'a> {
    pub types: Option<&'a Types>,
    pub env: &'a dyn Env,
}

impl Eval<'_> {
    fn types(&self) -> Result<&Types, String> {
        self.types.ok_or_else(|| "the kernel's types: none here".into())
    }

    /// Type `t`: its id (void: 0); for a pointer (one deep), what it points to.
    pub(super) fn type_of(&self, t: &TypeName) -> Result<(u32, Option<u32>), String> {
        let types = self.types()?;
        let base = if t.name == "void" { 0 } else { types.named(&t.name).ok_or_else(|| format!("no type {} in the kernel", t.name))? };
        Ok(if t.pointer { (0, Some(base)) } else { (base, None) })
    }

    fn eval(&self, e: &Expr) -> Result<V, String> {
        Ok(match e {
            Expr::Num(n) => V::Int(*n),
            Expr::Name(n) if n == "NULL" => V::Ptr { addr: 0, to: 0 },
            Expr::Name(n) => match self.env.sym(n) {
                Some(Sym::Value { v, to: None }) => V::Int(v),
                Some(Sym::Value { v, to: Some(to) }) => V::Ptr { addr: v as u32, to },
                Some(Sym::Var { addr, ty, count }) => V::Lv { addr, ty, count },
                None => return Err(format!("not a number, a constant or a name of the program's: {n}")),
            },
            Expr::Str(_) => return Err("a string: only as an argument, or a char array's value".into()),
            Expr::Init(_) => return Err("{…}: only as an argument, or a variable's value".into()),
            Expr::Unary("&", x) => match self.eval(x)? {
                V::Lv { addr, ty, .. } => V::Ptr { addr, to: ty },
                _ => return Err("&: of a variable, or a member of one".into()),
            },
            Expr::Unary(op, x) => {
                let n = num(self.rvalue(self.eval(x)?)?);
                V::Int(if *op == "-" { n.wrapping_neg() } else { !n })
            }
            Expr::Binary(op, l, r) => self.apply(op, self.rvalue(self.eval(l)?)?, self.rvalue(self.eval(r)?)?)?,
            Expr::Index(b, i) => {
                let i = num(self.rvalue(self.eval(i)?)?);
                match self.rvalue(self.eval(b)?)? {
                    V::Ptr { to: 0, .. } => return Err("[]: of a void *: name the result's type".into()),
                    V::Ptr { addr, to } => self.place((addr as i64 + i * self.types()?.size_of(to) as i64) as u32, to),
                    _ => return Err("[]: of a number, not an array or a pointer".into()),
                }
            }
        })
    }

    fn apply(&self, op: &str, l: V, r: V) -> Result<V, String> {
        // (Pointer arithmetic: by the size of what it points to; void's: 1.)
        let size = |to: u32| if to == 0 { Ok(1) } else { self.types().map(|t| t.size_of(to).max(1) as i64) };
        Ok(match (op, l, r) {
            ("+", V::Ptr { addr, to }, V::Int(i)) | ("+", V::Int(i), V::Ptr { addr, to }) => V::Ptr { addr: (addr as i64 + i * size(to)?) as u32, to },
            ("-", V::Ptr { addr, to }, V::Int(i)) => V::Ptr { addr: (addr as i64 - i * size(to)?) as u32, to },
            _ => {
                let (a, b) = (num(l), num(r));
                V::Int(match op {
                    "|" => a | b,
                    "^" => a ^ b,
                    "&" => a & b,
                    "<<" => a.checked_shl(b as u32).ok_or("a shift too far")?,
                    ">>" => a.checked_shr(b as u32).ok_or("a shift too far")?,
                    "+" => a.wrapping_add(b),
                    "-" => a.wrapping_sub(b),
                    "*" => a.wrapping_mul(b),
                    _ => a.checked_div(b).ok_or("a division by 0")?,
                })
            }
        })
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
        let mem = self.env.memory().ok_or("the program's memory: none here")?;
        let id = t.resolve(ty);
        match t.get(id).kind {
            Kind::Pointer(to) => Ok(V::Ptr { addr: mem.read_u32(addr), to: t.resolve(to) }),
            Kind::Base(_) | Kind::Enum(_) => {
                let mut b = [0u8; 8];
                mem.read(addr, &mut b[..(t.size_of(id) as usize).clamp(1, 8)]);
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

    /// Expression `e`'s value (a pointer's: its address).
    pub(super) fn number(&self, e: &Expr) -> Result<i64, String> {
        Ok(num(self.rvalue(self.eval(e)?)?))
    }
}

fn num(v: V) -> i64 {
    match v {
        V::Int(n) => n,
        V::Ptr { addr, .. } | V::Lv { addr, .. } => addr as i64,
    }
}

/// Expression `e`'s value (a pointer's: its address), with `types` for
/// members and memory (none: numbers and names only).
pub fn value(e: &Expr, types: Option<&Types>, env: &dyn Env) -> Result<i64, String> {
    Eval { types, env }.number(e)
}
