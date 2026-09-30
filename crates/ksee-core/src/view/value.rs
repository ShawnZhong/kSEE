//! A value in memory, as the page shows it (Object): a type's, read.

use super::*;

/// A value in memory. Aggregates beyond the requested depth have no
/// children yet (`expand` names the type to ask for them).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Object {
    pub name: String,
    pub ty: String,
    pub addr: u32,
    /// Its size in bytes.
    pub size: u32,
    pub value: Option<String>,
    pub children: Option<Vec<Object>>,
    /// Type id to expand this value (an aggregate not expanded yet).
    pub expand: Option<u32>,
    /// For a pointer to a struct or union: the target's type id.
    pub target: Option<u32>,
}

impl Types {
    /// The value of type `ty` at `addr`, aggregates expanded `depth` deep.
    pub fn object(&self, mem: &dyn Memory, name: &str, addr: u32, ty: u32, depth: u32) -> Object {
        self.object_bits(mem, name, addr, ty, None, depth)
    }

    /// The same, for a value that may be a bitfield: `bits`, its (offset in
    /// the byte at `addr`, size).
    fn object_bits(&self, mem: &dyn Memory, name: &str, addr: u32, ty: u32, bits: Option<(u32, u32)>, depth: u32) -> Object {
        let id = self.resolve(ty);
        let t = self.get(id);
        let mut o = Object {
            name: name.to_string(), ty: self.name(ty), addr, size: self.size_of(ty),
            value: None, children: None, expand: None, target: None,
        };
        match &t.kind {
            Kind::Struct(_) | Kind::Union(_) => {
                if depth == 0 {
                    o.expand = Some(id);
                } else {
                    // (Not one of size 0: nothing in it, a struct its
                    // config left empty, as rseq_data without CONFIG_RSEQ.)
                    o.children = Some(self.members(&t.kind).iter().filter(|m| m.bit_size > 0 || self.size_of(m.ty) > 0).map(|m| {
                        let bits = (m.bit_size > 0).then_some((m.bit_offset % 8, m.bit_size));
                        self.object_bits(mem, &m.name, addr + m.bit_offset / 8, m.ty, bits, depth - 1)
                    }).collect());
                }
            }
            Kind::Array { elem, count } if !self.is_string(*elem) => {
                if depth == 0 {
                    o.expand = Some(id);
                } else {
                    let esize = self.size_of(*elem);
                    o.children = Some((0..(*count).min(64)).map(|i| {
                        self.object_bits(mem, &format!("[{i}]"), addr + i * esize, *elem, None, depth - 1)
                    }).collect());
                }
            }
            Kind::Pointer(to) => {
                let p = mem.read_u32(addr);
                // (A char *: and the string it points to, if it looks like one: "0x1d40130 → \"/sub\"", as any pointer's target.)
                let text = if p != 0 && self.is_string(*to) { c_string(mem, p) } else { None };
                o.value = Some(match text {
                    _ if p == 0 => "NULL".into(),
                    Some(t) => format!("{p:#x} → {t}"),
                    None => format!("{p:#x}"),
                });
                let target = self.resolve(*to);
                match self.get(target).kind {
                    Kind::Struct(_) | Kind::Union(_) if p != 0 && self.size_of(target) > 0 => o.target = Some(target),
                    // (A pointer to a scalar: what it points to, in line: "0x7ffd8 → 0".)
                    Kind::Base(_) | Kind::Enum(_) | Kind::Pointer(_) if p != 0 && !self.is_string(*to) => {
                        let mut v = String::new();
                        self.print(mem, p, *to, 0, 0, &mut v);
                        o.value = Some(format!("{p:#x} → {v}"));
                    }
                    _ => {}
                }
            }
            _ => {
                let mut v = String::new();
                let (off, size) = bits.unwrap_or((0, 0));
                self.print(mem, addr, ty, off, size, &mut v);
                o.value = Some(v);
            }
        }
        o
    }
}
