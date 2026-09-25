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
//! S nr name params                       a syscall; params: comma-separated
//!                                        name:type:C type, "?" if unknown,
//!                                        "!" if not built
//! ```

use alloc::collections::BTreeMap;
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
    /// Syscalls, by number.
    pub syscalls: BTreeMap<u32, Syscall>,
    /// Functions' parameters: name -> [(parameter, type)].
    pub params: BTreeMap<String, Vec<(String, u32)>>,
}

/// A syscall's name and parameters (None if unknown).
#[derive(Debug, Clone, PartialEq)]
pub struct Syscall {
    pub name: String,
    /// Whether the kernel has it (not sys_ni_syscall).
    pub built: bool,
    pub params: Option<Vec<Param>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: u32,
    /// The C type as declared ("const char *").
    pub c_type: String,
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
                Some(&"S") if c.len() == 4 => {
                    let params = (c[3] != "?" && c[3] != "!").then(|| c[3].split(',').filter(|p| !p.is_empty()).map(|p| {
                        let mut f = p.splitn(3, ':');
                        Param {
                            name: f.next().unwrap_or("").to_string(),
                            ty: f.next().and_then(|t| t.parse().ok()).unwrap_or(0),
                            c_type: f.next().unwrap_or("?").to_string(),
                        }
                    }).collect());
                    syscalls.insert(num(1), Syscall { name: c[2].to_string(), built: c[3] != "!", params });
                }
                Some(&"P") if c.len() == 3 => {
                    let ps = c[2].split(',').filter(|p| !p.is_empty()).map(|p| {
                        let (n, t) = p.rsplit_once(':').unwrap_or((p, "0"));
                        (n.to_string(), t.parse().unwrap_or(0))
                    }).collect();
                    params.insert(c[1].to_string(), ps);
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
        Types { types, names, globals, syscalls, params }
    }

    pub fn get(&self, id: u32) -> &Type {
        static VOID: Type = Type { name: String::new(), size: 0, kind: Kind::Void };
        self.types.get(id as usize).unwrap_or(&VOID)
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

    /// Byte offset and size of `field` in struct `s` (a direct member).
    pub fn field(&self, s: &str, field: &str) -> Option<(u32, u32)> {
        let (Kind::Struct(m) | Kind::Union(m)) = &self.get(self.resolve(self.find(s)?)).kind else { return None };
        let m = m.iter().find(|m| m.name == field)?;
        Some((m.bit_offset / 8, self.get(m.ty).size.max(self.get(self.resolve(m.ty)).size)))
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

    /// Print the value of type `ty` at `addr`, GDB style, expanding nested
    /// aggregates `depth` levels deep.
    pub fn print(&self, mem: &dyn Memory, addr: u32, ty: u32, depth: u32, out: &mut String) {
        self.value(mem, addr, ty, 0, 0, depth, 0, out);
    }

    #[allow(clippy::too_many_arguments)]
    fn value(&self, mem: &dyn Memory, addr: u32, ty: u32, bit_off: u32, bit_size: u32,
             depth: u32, indent: usize, out: &mut String) {
        let id = self.resolve(ty);
        let t = self.get(id);
        match &t.kind {
            Kind::Base(b) => {
                let size = self.get(ty).size.max(t.size).min(8);
                let v = self.read_int(mem, addr, size, bit_off, bit_size);
                let signed = |v: u64, bits: u32| ((v << (64 - bits)) as i64 >> (64 - bits)) as i64;
                let bits = if bit_size > 0 { bit_size } else { size * 8 };
                let _ = match b {
                    Base::Signed if bits > 0 => write!(out, "{}", signed(v, bits)),
                    Base::Bool => write!(out, "{}", v != 0),
                    Base::Char => write!(out, "{}", signed(v, bits.max(1))),
                    Base::Float if size == 4 => write!(out, "{}", f32::from_bits(v as u32)),
                    Base::Float => write!(out, "{}", f64::from_bits(v)),
                    _ if v > 9 => write!(out, "{v:#x}"),
                    _ => write!(out, "{v}"),
                };
            }
            Kind::Enum(values) => {
                let v = self.read_int(mem, addr, t.size.min(8), bit_off, bit_size) as i64;
                match values.iter().find(|(_, x)| *x == v) {
                    Some((name, _)) => out.push_str(name),
                    None => {
                        let _ = write!(out, "{v}");
                    }
                }
            }
            Kind::Pointer(_) => {
                let p = mem.read_u32(addr);
                let _ = if p == 0 { write!(out, "NULL") } else { write!(out, "{p:#x}") };
            }
            Kind::Array { elem, count } => {
                let e = self.get(self.resolve(*elem));
                let esize = self.size_of(*elem);
                if matches!(e.kind, Kind::Base(Base::Char)) && esize == 1 {
                    let mut buf = alloc::vec![0u8; (*count).min(256) as usize];
                    mem.read(addr, &mut buf);
                    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                    let _ = write!(out, "{:?}", String::from_utf8_lossy(&buf[..len]));
                    return;
                }
                if depth == 0 {
                    out.push_str("[...]");
                    return;
                }
                out.push('[');
                let shown = (*count).min(16);
                for i in 0..shown {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    self.value(mem, addr + i * esize, *elem, 0, 0, depth - 1, indent, out);
                }
                if *count > shown {
                    let _ = write!(out, ", ... ({count} total)");
                }
                out.push(']');
            }
            Kind::Struct(members) | Kind::Union(members) => {
                if depth == 0 {
                    out.push_str("{...}");
                    return;
                }
                out.push_str("{\n");
                for m in members {
                    for _ in 0..indent + 1 {
                        out.push_str("  ");
                    }
                    let name = if m.name.is_empty() { "<anon>" } else { &m.name };
                    let _ = write!(out, "{name} = ");
                    let byte = m.bit_offset / 8;
                    let (boff, bsize) = if m.bit_size > 0 { (m.bit_offset % 8, m.bit_size) } else { (0, 0) };
                    self.value(mem, addr + byte, m.ty, boff, bsize, depth - 1, indent + 1, out);
                    out.push_str(",\n");
                }
                for _ in 0..indent {
                    out.push_str("  ");
                }
                out.push('}');
            }
            Kind::Func => out.push_str("<function>"),
            Kind::Void | Kind::Typedef(_) => out.push_str("<void>"),
        }
    }

    /// Print a scalar (or string) value at `addr`, possibly a bitfield.
    pub(crate) fn print_bits(&self, mem: &dyn Memory, addr: u32, ty: u32, bit_off: u32,
                             bit_size: u32, out: &mut String) {
        self.value(mem, addr, ty, bit_off, bit_size, 0, 0, out);
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

    fn read_int(&self, mem: &dyn Memory, addr: u32, size: u32, bit_off: u32, bit_size: u32) -> u64 {
        let bytes = if bit_size > 0 { (bit_off + bit_size).div_ceil(8) } else { size }.min(8);
        let mut buf = [0u8; 8];
        mem.read(addr, &mut buf[..bytes as usize]);
        let v = u64::from_le_bytes(buf);
        if bit_size > 0 {
            (v >> bit_off) & ((1u64 << bit_size) - 1)
        } else {
            v
        }
    }
}

fn or_anon(name: &str) -> &str {
    if name.is_empty() { "<anon>" } else { name }
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
        let mut out = String::new();
        t.print(&Mem(m), 0, 6, 2, &mut out);
        assert_eq!(out, "{\n  pid = -3,\n  comm = \"init\",\n  next = 0x40,\n  flags = 5,\n}");
    }
}
