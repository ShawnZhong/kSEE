//! Kernel types (from DWARF, see `ksee pack`) and decoding memory with them.
//!
//! The `ksee.types` custom section lists types by dense id (0 is void),
//! one per line, tab-separated:
//! ```text
//! T id kind name size [extra]   kind: base (extra s|u|f|b|c), ptr (target),
//!                               array (elem count), struct, union, enum,
//!                               typedef (target), func
//! M id name bit_offset bit_size type     a member of struct/union id
//! V id name value                        an enumerator of enum id
//! G name address type                    a variable with a fixed address
//! W name file line                       where a function, type or variable is declared
//! S nr name                              a syscall this kernel builds
//!                                        (its parameters: its body's P line)
//! C name value                           a constant of the UAPI headers
//! P fn name param:type,…                 a function's parameters (fn: its
//!                                        id, as the probes have it)
//! Q fn var:type:offset,…                 its locals (the offset, from the
//!                                        frame base, if in memory)
//! ```

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt::Write;

use crate::Memory;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Base {
    Signed,
    Unsigned,
    Float,
    Bool,
    Char,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Member {
    pub name: String,
    pub bit_offset: u32,
    /// 0 unless a bitfield.
    pub bit_size: u32,
    pub ty: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Void,
    Base(Base),
    Pointer(u32),
    Array { elem: u32, count: u32 },
    Struct(Vec<Member>),
    Union(Vec<Member>),
    Enum(Vec<(String, i64)>),
    Typedef(u32),
    Func,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Type {
    pub name: String,
    pub size: u32,
    pub kind: Kind,
}

#[derive(Debug, Default)]
pub struct Types {
    pub types: Vec<Type>,
    /// Struct, union, enum and typedef names -> ids.
    names: BTreeMap<String, u32>,
    /// Variables with fixed addresses: name -> (address, type).
    pub globals: BTreeMap<String, (u32, u32)>,
    /// Syscall names, by number.
    pub syscalls: BTreeMap<u32, String>,
    /// Functions' parameters, by function id (the probes' F lines'): [(parameter, type)].
    pub params: BTreeMap<u32, Vec<(String, u32)>>,
    /// The function ids of the names (the first of a name: for the
    /// syscalls' bodies, looked for by name).
    by_name: BTreeMap<String, u32>,
    /// Functions' local variables, by function id: [(variable, type,
    /// offset from the frame base, if it is in memory)].
    pub locals: BTreeMap<u32, Vec<(String, u32, Option<u32>)>>,
    /// Constants from the UAPI headers (flags, errno names): name -> value.
    pub constants: BTreeMap<String, i64>,
    /// Where a function (the inline ones too, which have no probe), a type
    /// (struct, union, enum, typedef) or a global is declared: name ->
    /// (file, line).
    pub declared: BTreeMap<String, (String, u32)>,
}

impl Types {
    pub fn parse(text: &str) -> Types {
        let mut types = Vec::new();
        let mut kinds: Vec<(String, Vec<String>)> = Vec::new();
        let mut members: BTreeMap<u32, Vec<Member>> = BTreeMap::new();
        let mut values: BTreeMap<u32, Vec<(String, i64)>> = BTreeMap::new();
        let mut globals = BTreeMap::new();
        let mut syscalls = BTreeMap::new();
        let mut params = BTreeMap::new();
        let mut by_name = BTreeMap::new();
        let mut locals = BTreeMap::new();
        let mut constants = BTreeMap::new();
        let mut declared = BTreeMap::new();
        for line in text.lines() {
            let c: Vec<&str> = line.split('\t').collect();
            let num = |i: usize| c.get(i).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
            match c.first() {
                Some(&"T") if c.len() >= 5 => {
                    let id = num(1) as usize;
                    if types.len() <= id {
                        types.resize(id + 1, Type { name: String::new(), size: 0, kind: Kind::Void });
                        kinds.resize(id + 1, (String::new(), Vec::new()));
                    }
                    types[id].name = c[3].to_string();
                    types[id].size = num(4);
                    kinds[id] = (c[2].to_string(), c[5..].iter().map(|s| s.to_string()).collect());
                }
                Some(&"M") if c.len() == 6 => members.entry(num(1)).or_default().push(Member {
                    name: c[2].to_string(), bit_offset: num(3), bit_size: num(4), ty: num(5),
                }),
                Some(&"S") if c.len() >= 3 => {
                    syscalls.insert(num(1), c[2].to_string());
                }
                Some(&"C") if c.len() == 3 => {
                    if let Ok(v) = c[2].parse() {
                        constants.insert(c[1].to_string(), v);
                    }
                }
                // (P: id, name, then name:type for each parameter.)
                Some(&"P") if c.len() == 4 => {
                    let list = c[3].split(',').filter(|p| !p.is_empty()).map(|p| {
                        let (n, t) = p.rsplit_once(':').unwrap_or((p, "0"));
                        (n.to_string(), t.parse().unwrap_or(0))
                    }).collect();
                    params.insert(num(1), list);
                    by_name.entry(c[2].to_string()).or_insert(num(1));
                }
                // (Q: name:type:offset for each local, the offset empty
                // for one not in memory.)
                Some(&"Q") if c.len() == 3 => {
                    let list = c[2].split(',').filter_map(|p| {
                        let mut f = p.split(':');
                        let (n, t, o) = (f.next()?, f.next()?, f.next()?);
                        // (Not one the kernel's macros made up.)
                        if n.starts_with(crate::arch::MADE_UP) {
                            return None;
                        }
                        Some((n.to_string(), t.parse().ok()?, o.parse().ok()))
                    }).collect();
                    locals.insert(num(1), list);
                }
                Some(&"W") if c.len() == 4 => {
                    declared.entry(c[1].to_string()).or_insert((c[2].to_string(), num(3)));
                }
                Some(&"G") if c.len() == 4 => {
                    globals.insert(c[1].to_string(), (num(2), num(3)));
                }
                Some(&"V") if c.len() == 4 => {
                    values.entry(num(1)).or_default().push((c[2].to_string(), c[3].parse().unwrap_or(0)))
                }
                _ => {}
            }
        }
        let mut names = BTreeMap::new();
        for (id, (kind, extra)) in kinds.into_iter().enumerate() {
            let arg = |i: usize| extra.get(i).and_then(|s| s.parse().ok()).unwrap_or(0);
            let t = &mut types[id];
            t.kind = match kind.as_str() {
                "base" => Kind::Base(match extra.first().map(String::as_str) {
                    Some("s") => Base::Signed,
                    Some("f") => Base::Float,
                    Some("b") => Base::Bool,
                    Some("c") => Base::Char,
                    _ => Base::Unsigned,
                }),
                "ptr" => Kind::Pointer(arg(0)),
                "array" => Kind::Array { elem: arg(0), count: arg(1) },
                "struct" => Kind::Struct(members.remove(&(id as u32)).unwrap_or_default()),
                "union" => Kind::Union(members.remove(&(id as u32)).unwrap_or_default()),
                "enum" => Kind::Enum(values.remove(&(id as u32)).unwrap_or_default()),
                "typedef" => Kind::Typedef(arg(0)),
                "func" => Kind::Func,
                _ => Kind::Void,
            };
            if matches!(t.kind, Kind::Struct(_) | Kind::Union(_) | Kind::Enum(_) | Kind::Typedef(_))
                && !t.name.is_empty()
            {
                names.entry(t.name.clone()).or_insert(id as u32);
            }
        }
        Types { types, names, globals, syscalls, params, by_name, locals, constants, declared }
    }

    pub fn get(&self, id: u32) -> &Type {
        static VOID: Type = Type { name: String::new(), size: 0, kind: Kind::Void };
        self.types.get(id as usize).unwrap_or(&VOID)
    }

    /// Syscall `name`'s parameters (its body's, arch::SYSCALL_BODY).
    pub fn syscall_params(&self, name: &str) -> Option<&Vec<(String, u32)>> {
        self.params.get(self.by_name.get(&format!("{}{name}", crate::arch::SYSCALL_BODY))?)
    }

    /// Every pointer in a value of type `ty` (it, or in the structs,
    /// unions and arrays it holds, however deep: a value holds itself in
    /// none): each one's path ("f_path.dentry", "fd_array[3]"; "" itself)
    /// and offset.
    pub fn pointers(&self, ty: u32) -> Vec<(String, u32)> {
        let t = self.get(self.resolve(ty));
        match &t.kind {
            Kind::Pointer(_) => alloc::vec![(String::new(), 0)],
            Kind::Struct(_) | Kind::Union(_) => self.members(&t.kind).iter().filter(|m| m.bit_size == 0).flat_map(|m| {
                self.pointers(m.ty).into_iter().map(|(p, off)| {
                    (if p.is_empty() || p.starts_with('[') { alloc::format!("{}{p}", m.name) } else { alloc::format!("{}.{p}", m.name) }, m.bit_offset / 8 + off)
                }).collect::<Vec<_>>()
            }).collect(),
            &Kind::Array { elem, count } => {
                let (inner, size) = (self.pointers(elem), self.size_of(elem));
                if inner.is_empty() {
                    return Vec::new();
                }
                (0..count).flat_map(|i| inner.iter().map(move |(p, off)| {
                    (if p.is_empty() || p.starts_with('[') { alloc::format!("[{i}]{p}") } else { alloc::format!("[{i}].{p}") }, i * size + off)
                })).collect()
            }
            _ => Vec::new(),
        }
    }

    /// Syscall `nr`'s name, or its number if this kernel has none.
    pub fn syscall_name(&self, nr: u32) -> String {
        self.syscalls.get(&nr).cloned().unwrap_or_else(|| nr.to_string())
    }

    /// A call's result as the page says it: a number (in hex past 0xffff:
    /// a pointer, a flag word), or a failure with its errno's name, "-2
    /// (ENOENT)".
    pub fn result(&self, v: i64) -> String {
        match v {
            0x10000.. => alloc::format!("{v:#x}"),
            0.. => alloc::format!("{v}"),
            _ => alloc::format!("{v} ({})", self.errno(-v).unwrap_or("an error")),
        }
    }

    /// Syscall `name`'s parameters, each as C declares it: ["unsigned int
    /// fd"] for close; none if this kernel has no syscall `name`.
    pub fn params_of(&self, name: &str) -> Option<Vec<String>> {
        self.syscalls.values().find(|n| n.as_str() == name)?;
        Some(self.syscall_params(name).into_iter().flatten().map(|(p, ty)| {
            let t = self.name(*ty);
            alloc::format!("{t}{}{p}", if t.ends_with('*') { "" } else { " " })
        }).collect())
    }

    /// Syscall `name`'s signature, as C declares it: "close(unsigned int fd)".
    pub fn signature(&self, name: &str) -> Option<String> {
        Some(alloc::format!("{name}({})", self.params_of(name)?.join(", ")))
    }

    /// The errno name of `n` (a failed call's result is -n): ENOENT for 2.
    pub fn errno(&self, n: i64) -> Option<&str> {
        let is = |k: &str| k.strip_prefix(crate::arch::ERRNO).is_some_and(|r| !r.is_empty() && r.bytes().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()));
        (1..=crate::arch::MAX_ERRNO).contains(&n).then(|| self.constants.iter().find(|&(k, &v)| v == n && is(k)).map(|(k, _)| k.as_str())).flatten()
    }

    /// A struct, union, enum or typedef by name ("task_struct").
    pub fn find(&self, name: &str) -> Option<u32> {
        self.names.get(name).copied()
    }

    /// Follow typedefs.
    pub fn resolve(&self, mut id: u32) -> u32 {
        for _ in 0..32 {
            match self.get(id).kind {
                Kind::Typedef(to) => id = to,
                _ => break,
            }
        }
        id
    }

    /// Byte offset and size of `field` in struct `s` (a member, or one of
    /// an anonymous struct's or union's in it: see members).
    pub fn field(&self, s: &str, field: &str) -> Option<(u32, u32)> {
        let m = self.members(&self.get(self.resolve(self.find(s)?)).kind).into_iter().find(|m| m.name == field)?;
        Some((m.bit_offset / 8, self.get(m.ty).size.max(self.get(self.resolve(m.ty)).size)))
    }

    /// A struct's or union's members, as C names them: an anonymous
    /// struct's or union's own members in its place (their offsets from
    /// the outer one's start).
    pub fn members(&self, kind: &Kind) -> Vec<Member> {
        let (Kind::Struct(members) | Kind::Union(members)) = kind else { return Vec::new() };
        let mut out = Vec::with_capacity(members.len());
        for m in members {
            let inner = &self.get(self.resolve(m.ty)).kind;
            if m.name.is_empty() && matches!(inner, Kind::Struct(_) | Kind::Union(_)) {
                out.extend(self.members(inner).into_iter().map(|i| Member { bit_offset: m.bit_offset + i.bit_offset, ..i }));
            } else {
                out.push(m.clone());
            }
        }
        out
    }

    /// The type's C name: "struct task_struct *", "unsigned int[16]".
    pub fn name(&self, id: u32) -> String {
        let t = self.get(id);
        match &t.kind {
            Kind::Void => "void".into(),
            Kind::Pointer(to) => {
                let inner = self.name(*to);
                if inner.ends_with('*') { inner + "*" } else { inner + " *" }
            }
            Kind::Array { elem, count } => alloc::format!("{}[{count}]", self.name(*elem)),
            Kind::Struct(_) => alloc::format!("struct {}", or_anon(&t.name)),
            Kind::Union(_) => alloc::format!("union {}", or_anon(&t.name)),
            Kind::Enum(_) => alloc::format!("enum {}", or_anon(&t.name)),
            Kind::Func => "func".into(),
            _ => t.name.clone(),
        }
    }

    /// Whether `ty` holds flags (arch::FLAG_TYPES: fmode_t, gfp_t,
    /// vm_flags_t, ...).
    fn is_flags(&self, ty: u32) -> bool {
        let name = self.name(ty);
        crate::arch::FLAG_TYPES.iter().any(|&f| name == f || f.starts_with('*') && name.ends_with(&f[1..]))
    }

    /// Print the scalar (or string) value of type `ty` at `addr`, possibly
    /// a bitfield (`bit_off`, `bit_size`); an aggregate as "{...}" (see
    /// view::Object for their members).
    pub fn print(&self, mem: &dyn Memory, addr: u32, ty: u32, bit_off: u32, bit_size: u32) -> String {
        let mut out = String::new();
        let id = self.resolve(ty);
        let t = self.get(id);
        match &t.kind {
            Kind::Base(b) => {
                let size = self.get(ty).size.max(t.size).min(8);
                let v = self.read_int(mem, addr, size, bit_off, bit_size);
                let signed = |v: u64, bits: u32| (v << (64 - bits)) as i64 >> (64 - bits);
                let bits = if bit_size > 0 { bit_size } else { size * 8 };
                let _ = match b {
                    Base::Signed if bits > 0 => write!(&mut out, "{}", signed(v, bits)),
                    Base::Bool => write!(&mut out, "{}", v != 0),
                    // (A char: its number, and the character if it is one.)
                    Base::Char if (0x20..0x7f).contains(&v) => write!(&mut out, "{} '{}'", v, v as u8 as char),
                    Base::Char => write!(&mut out, "{}", signed(v, bits.max(1))),
                    Base::Float if size == 4 => write!(&mut out, "{}", f32::from_bits(v as u32)),
                    Base::Float => write!(&mut out, "{}", f64::from_bits(v)),
                    // Unsigned: decimal, but flags in hex and file modes in octal.
                    _ if v > 9 && self.is_flags(ty) => write!(&mut out, "{v:#x}"),
                    _ if v > 7 && crate::arch::OCTAL_TYPES.contains(&self.name(ty).as_str()) => write!(&mut out, "0{v:o}"),
                    _ => write!(&mut out, "{v}"),
                };
            }
            Kind::Enum(values) => {
                // (An enumerator may be negative: as the value's bits, or signed.)
                let bits = if bit_size > 0 { bit_size } else { t.size.min(8) * 8 };
                let u = self.read_int(mem, addr, t.size.min(8), bit_off, bit_size);
                let v = if bits > 0 && bits < 64 { (u << (64 - bits)) as i64 >> (64 - bits) } else { u as i64 };
                match values.iter().find(|(_, x)| *x == v || *x == u as i64) {
                    Some((name, _)) => out.push_str(name),
                    None => {
                        let _ = write!(&mut out, "{v}");
                    }
                }
            }
            Kind::Pointer(_) => {
                let p = mem.read_u32(addr);
                let _ = if p == 0 { write!(&mut out, "NULL") } else { write!(&mut out, "{p:#x}") };
            }
            // (A char array: its string.)
            Kind::Array { elem, count } if self.is_string(*elem) => {
                let mut buf = alloc::vec![0u8; (*count).min(256) as usize];
                mem.read(addr, &mut buf);
                let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                out.push_str(&quote(&buf[..len]));
            }
            Kind::Array { .. } | Kind::Struct(_) | Kind::Union(_) => out.push_str("{...}"),
            Kind::Func => out.push_str("<function>"),
            Kind::Void | Kind::Typedef(_) => out.push_str("<void>"),
        }
        out
    }

    /// If `ty` is a pointer to a struct or a union: what it points to.
    pub fn struct_target(&self, ty: u32) -> Option<u32> {
        let Kind::Pointer(to) = self.get(self.resolve(ty)).kind else { return None };
        matches!(self.get(self.resolve(to)).kind, Kind::Struct(_) | Kind::Union(_)).then_some(to)
    }

    /// Whether `elem` is a char (so an array or pointer of it is a string).
    pub fn is_string(&self, elem: u32) -> bool {
        matches!(self.get(self.resolve(elem)).kind, Kind::Base(Base::Char)) && self.size_of(elem) == 1
    }

    /// The scalar (or string) at byte `off` of a value of type `ty`: its
    /// path in the value ("f_path.dentry", "[2]"; "" for the value itself),
    /// its type, its offset in the value, and its bits if a bitfield
    /// (offset in its first byte, size).
    pub fn leaf(&self, ty: u32, off: u32) -> (String, u32, u32, Option<(u32, u32)>) {
        let (mut path, mut ty, mut at, mut bits) = (String::new(), ty, 0, None);
        loop {
            match &self.get(self.resolve(ty)).kind {
                k @ (Kind::Struct(_) | Kind::Union(_)) => {
                    // (A member of no size, an empty struct such as a spinlock
                    // on one CPU, shares its offset with the next: not it.)
                    let Some(m) = self.members(k).into_iter()
                        .find(|m| self.size_of(m.ty) > 0 && m.bit_offset / 8 <= off - at && off - at < m.bit_offset / 8 + self.size_of(m.ty)) else { break };
                    if !path.is_empty() {
                        path.push('.');
                    }
                    path += &m.name;
                    at += m.bit_offset / 8;
                    ty = m.ty;
                    bits = (m.bit_size > 0).then_some((m.bit_offset % 8, m.bit_size));
                }
                &Kind::Array { elem, count } if !self.is_string(elem) => {
                    let k = (off - at) / self.size_of(elem).max(1);
                    if k >= count {
                        break;
                    }
                    path += &alloc::format!("[{k}]");
                    at += k * self.size_of(elem);
                    ty = elem;
                }
                _ => break,
            }
        }
        (path, ty, at, bits)
    }

    /// The scalars (or strings) bytes `off..off + len` of a value of type
    /// `ty` are in, in order (see leaf; padding between them skipped; up to
    /// 16): a struct copied by one wide store writes each of its members.
    pub fn leaves(&self, ty: u32, off: u32, len: u32) -> Vec<(String, u32, u32, Option<(u32, u32)>)> {
        let (mut out, mut o, end) = (Vec::new(), off, off.saturating_add(len.max(1)));
        while o < end && out.len() < 16 {
            let l @ (_, leaf, at, _) = self.leaf(ty, o);
            // (A byte no scalar holds: padding, an empty member.)
            if matches!(self.get(self.resolve(leaf)).kind, Kind::Struct(_) | Kind::Union(_)) || at + self.size_of(leaf) <= o {
                o += 1;
                continue;
            }
            o = at + self.size_of(leaf);
            out.push(l);
        }
        out
    }

    /// Size in bytes of type `ty` (arrays: count * element).
    pub fn size_of(&self, ty: u32) -> u32 {
        let t = self.get(self.resolve(ty));
        match t.kind {
            Kind::Array { elem, count } if t.size == 0 => count * self.size_of(elem),
            Kind::Pointer(_) => 4,
            _ => t.size,
        }
    }

    /// A type's layout, pahole style: each member with its offset and size.
    pub fn layout(&self, id: u32) -> String {
        let t = self.get(self.resolve(id));
        let mut out = alloc::format!("{} {{\t/* size {} */\n", self.name(self.resolve(id)), t.size);
        match &t.kind {
            Kind::Struct(members) | Kind::Union(members) => {
                for m in members {
                    let bits = if m.bit_size > 0 { alloc::format!(":{}", m.bit_size) } else { String::new() };
                    let decl = alloc::format!("{} {}{bits};", self.name(m.ty), m.name);
                    let _ = writeln!(out, "\t{decl:<48}\t/* {:>5} {:>5} */", m.bit_offset / 8, self.size_of(m.ty));
                }
            }
            Kind::Enum(values) => {
                for (name, v) in values {
                    let _ = writeln!(out, "\t{name} = {v},");
                }
            }
            _ => {}
        }
        out.push_str("};\n");
        out
    }

    pub(crate) fn read_int(&self, mem: &dyn Memory, addr: u32, size: u32, bit_off: u32, bit_size: u32) -> u64 {
        let bytes = if bit_size > 0 { (bit_off + bit_size).div_ceil(8) } else { size }.min(8);
        let mut buf = [0u8; 8];
        mem.read(addr, &mut buf[..bytes as usize]);
        let v = u64::from_le_bytes(buf);
        if bit_size > 0 {
            (v >> bit_off) & (u64::MAX >> (64 - bit_size.min(64)))
        } else {
            v
        }
    }
}

fn or_anon(name: &str) -> &str {
    if name.is_empty() { "<anon>" } else { name }
}

/// Bytes as a C string literal: text as text, the rest escaped (\n, \t,
/// \", \\, \0, \xHH); past 64, "…".
pub fn quote(b: &[u8]) -> String {
    let mut s = String::from("\"");
    for &c in b.iter().take(64) {
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
    if b.len() > 64 {
        s.push('…');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Mem(Vec<u8>);
    impl Memory for Mem {
        fn read(&self, addr: u32, buf: &mut [u8]) {
            buf.copy_from_slice(&self.0[addr as usize..addr as usize + buf.len()]);
        }
    }

    const TYPES: &str = "T\t1\tbase\tint\t4\ts\n\
                         T\t2\tbase\tchar\t1\tc\n\
                         T\t3\tarray\t\t8\t2\t8\n\
                         T\t4\tstruct\ttask\t20\n\
                         M\t4\tpid\t0\t0\t1\n\
                         M\t4\tcomm\t32\t0\t3\n\
                         M\t4\tnext\t96\t0\t5\n\
                         M\t4\tflags\t128\t3\t7\n\
                         T\t5\tptr\t\t4\t4\n\
                         T\t6\ttypedef\ttask_t\t20\t4\n\
                         T\t7\tbase\tunsigned int\t4\tu\n";

    #[test]
    fn anonymous_members_are_the_outer_ones() {
        // struct page { int a; union { int b; struct { int c; int d; }; }; } at bit 32, and c, d at 0, 32 in it.
        let t = Types::parse("T\t1\tbase\tint\t4\ts\nT\t2\tstruct\tpage\t12\nM\t2\ta\t0\t0\t1\nM\t2\t\t32\t0\t3\n\
                              T\t3\tunion\t\t8\nM\t3\tb\t0\t0\t1\nM\t3\t\t0\t0\t4\n\
                              T\t4\tstruct\t\t8\nM\t4\tc\t0\t0\t1\nM\t4\td\t32\t0\t1\n");
        let names: Vec<_> = t.members(&t.get(2).kind).into_iter().map(|m| (m.name, m.bit_offset)).collect();
        assert_eq!(names, [("a".into(), 0), ("b".into(), 32), ("c".into(), 32), ("d".into(), 64)]);
        assert_eq!(t.field("page", "d"), Some((8, 4)));
    }

    #[test]
    fn a_byte_is_in_a_member() {
        let t = Types::parse(TYPES);
        assert_eq!(t.leaf(4, 6), ("comm".into(), 3, 4, None));
        assert_eq!(t.leaf(4, 17), ("flags".into(), 7, 16, Some((0, 3))));
        assert_eq!(t.leaf(1, 2), ("".into(), 1, 0, None));
        // An empty struct (a lock on one CPU) at 4, and int next_fd at 4 too: the int.
        let t = Types::parse("T\t1\tbase\tint\t4\ts\nT\t2\tstruct\tlock\t0\nT\t3\tstruct\tfiles\t8\nM\t3\tcount\t0\t0\t1\nM\t3\tlock\t32\t0\t2\nM\t3\tnext_fd\t32\t0\t1\n");
        assert_eq!(t.leaf(3, 4), ("next_fd".into(), 1, 4, None));
    }

    #[test]
    fn a_wide_store_writes_each_member_it_covers() {
        // struct path { void *mnt; void *dentry; } copied by one 8-byte store.
        let t = Types::parse("T\t1\tptr\t\t4\t0\nT\t2\tstruct\tpath\t8\nM\t2\tmnt\t0\t0\t1\nM\t2\tdentry\t32\t0\t1\n");
        let got: Vec<_> = t.leaves(2, 0, 8).into_iter().map(|(p, _, off, _)| (p, off)).collect();
        assert_eq!(got, [("mnt".into(), 0), ("dentry".into(), 4)]);
        assert_eq!(t.leaves(2, 4, 4).len(), 1);
    }

    #[test]
    fn finds_fields() {
        let t = Types::parse(TYPES);
        assert_eq!(t.field("task", "comm"), Some((4, 8)));
        assert_eq!(t.field("task_t", "next"), Some((12, 4)));
        assert_eq!(t.name(5), "struct task *");
    }

    #[test]
    fn prints_values() {
        let t = Types::parse(TYPES);
        let mut m = alloc::vec![0u8; 32];
        m[0..4].copy_from_slice(&(-3i32).to_le_bytes());
        m[4..8].copy_from_slice(b"init");
        m[12..16].copy_from_slice(&0x40u32.to_le_bytes());
        m[16] = 0b0101; // flags: 3 bits = 5
        let mem = Mem(m);
        let at = |addr, ty, off, size| {
            let out = t.print(&mem, addr, ty, off, size);
            out
        };
        assert_eq!((at(0, 1, 0, 0), at(4, 3, 0, 0), at(12, 5, 0, 0), at(16, 7, 0, 3), at(0, 6, 0, 0)),
                   ("-3".into(), "\"init\"".into(), "0x40".into(), "5".into(), "{...}".into()));
    }


    #[test]
    fn every_pointer_in_a_value_however_deep() {
        // struct s { int a; struct in { void *p; } in; void *arr[2]; }
        let t = Types::parse("T\t1\tbase\tint\t4\ts\nT\t2\tptr\t\t4\t0\nT\t3\tstruct\tin\t4\nM\t3\tp\t0\t0\t2\n\
                              T\t4\tarray\t\t8\t2\t2\nT\t5\tstruct\ts\t16\nM\t5\ta\t0\t0\t1\nM\t5\tin\t32\t0\t3\nM\t5\tarr\t64\t0\t4\n");
        assert_eq!(t.pointers(5), [("in.p".into(), 4), ("arr[0]".into(), 8), ("arr[1]".into(), 12)]);
    }
}
