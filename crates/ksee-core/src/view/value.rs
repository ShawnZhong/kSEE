//! A value in memory, as the page shows it (Object): a type's, read.

use super::*;

/// A value in memory. Aggregates beyond the requested depth have no
/// children yet (`open` says what to ask for them).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Object {
    pub name: String,
    pub ty: String,
    /// Its type's id (typedefs resolved).
    pub id: u32,
    pub addr: u32,
    /// Its size in bytes.
    pub size: u32,
    pub value: Option<String>,
    /// Why it has none, if (its `value` says it too, in parentheses).
    pub none: Option<Missing>,
    pub children: Option<Vec<Object>>,
    /// What opening it reads (its children not read yet): itself, an
    /// aggregate; or what it points to, a pointer to a struct or union not
    /// NULL.
    pub open: Option<Target>,
}

/// How many elements of an array a value shows (then a row: how many more).
const SHOWN: u32 = 64;

/// What a pointer points to: its type's id, and its address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub ty: u32,
    pub addr: u32,
}

/// Why a value has none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Missing {
    /// Not stored to yet.
    Uninitialized,
    /// The recording doesn't have it.
    NotRecorded,
    /// The optimizer took it away.
    OptimizedOut,
}

impl Missing {
    /// As its value says it: "(uninitialized)".
    pub fn text(self) -> &'static str {
        match self {
            Missing::Uninitialized => "(uninitialized)",
            Missing::NotRecorded => "(not recorded)",
            Missing::OptimizedOut => "(optimized out)",
        }
    }
}

impl Object {
    /// Without a value, for `why`.
    pub(crate) fn missing(self, why: Missing) -> Object {
        Object { value: Some(why.text().into()), none: Some(why), children: None, open: None, ..self }
    }
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
            name: name.to_string(), ty: self.name(ty), id, addr, size: self.size_of(ty),
            value: None, none: None, children: None, open: None,
        };
        match &t.kind {
            Kind::Struct(_) | Kind::Union(_) => {
                if depth == 0 {
                    o.open = Some(Target { ty: id, addr });
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
                    o.open = Some(Target { ty: id, addr });
                } else {
                    let esize = self.size_of(*elem);
                    let mut shown: Vec<Object> = (0..(*count).min(SHOWN)).map(|i| {
                        self.object_bits(mem, &format!("[{i}]"), addr + i * esize, *elem, None, depth - 1)
                    }).collect();
                    // (Past the first SHOWN: a row saying how many more.)
                    if *count > SHOWN {
                        shown.push(Object { name: format!("[{SHOWN}…{}]", count - 1), ty: String::new(), id: 0, addr: addr + SHOWN * esize, size: 0,
                                            value: Some(format!("… {} more", count - SHOWN)), none: None, children: None, open: None });
                    }
                    o.children = Some(shown);
                }
            }
            Kind::Pointer(to) => {
                let p = mem.read_u32(addr);
                // (A char *: and the string it points to: "0x1d40130 → \"/sub\"", as any pointer's target.)
                let text = (p != 0 && self.is_string(*to)).then(|| c_string(mem, p));
                o.value = Some(match text {
                    _ if p == 0 => "NULL".into(),
                    Some(t) => format!("{p:#x} → {t}"),
                    None => format!("{p:#x}"),
                });
                let target = self.resolve(*to);
                match self.get(target).kind {
                    Kind::Struct(_) | Kind::Union(_) if p != 0 && self.size_of(target) > 0 => o.open = Some(Target { ty: target, addr: p }),
                    // (A pointer to a scalar: what it points to, in line: "0x7ffd8 → 0".)
                    Kind::Base(_) | Kind::Enum(_) | Kind::Pointer(_) if p != 0 && !self.is_string(*to) => {
                        let v = self.print(mem, p, *to, 0, 0);
                        o.value = Some(format!("{p:#x} → {v}"));
                    }
                    _ => {}
                }
            }
            _ => {
                let (off, size) = bits.unwrap_or((0, 0));
                o.value = Some(self.print(mem, addr, ty, off, size));
            }
        }
        o
    }
}
