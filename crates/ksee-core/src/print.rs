//! Values of the kernel's types, read from memory and printed as C would
//! write them (the views', the program's): a scalar, a C string, a task.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt::Write;

use crate::types::{Base, Kind, Types};
use crate::Memory;

/// A task's id and name, as its task_struct says them.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskId {
    pub pid: i32,
    pub comm: String,
}

impl Types {
    /// A task_struct's pid and comm, at `p` in `mem`.
    pub fn read_task(&self, mem: &dyn Memory, p: u32) -> Option<TaskId> {
        let (pid, comm) = (self.field(crate::arch::TASK, crate::arch::TASK_PID)?, self.field(crate::arch::TASK, crate::arch::TASK_COMM)?);
        if p == 0 {
            return None;
        }
        let comm = c_str(mem, p + comm.0, comm.1);
        Some(TaskId { pid: mem.read_u32(p + pid.0) as i32, comm: String::from_utf8_lossy(&comm).into() })
    }

    /// Print the scalar (or string) value of type `ty` at `addr`, possibly
    /// a bitfield (`bit_off`, `bit_size`); an aggregate as "{...}" (see
    /// view::Object for their members).
    pub(crate) fn print(&self, mem: &dyn Memory, addr: u32, ty: u32, bit_off: u32, bit_size: u32) -> String {
        let mut out = String::new();
        let id = self.resolve(ty);
        let t = self.get(id);
        match &t.kind {
            Kind::Base(b) => {
                let size = self.size_of(ty).min(8);
                let v = self.read_int(mem, addr, size, bit_off, bit_size);
                let bits = if bit_size > 0 { bit_size } else { size * 8 };
                let _ = match b {
                    Base::Signed if bits > 0 => write!(&mut out, "{}", sign_extend(v, bits)),
                    Base::Bool => write!(&mut out, "{}", v != 0),
                    // (A char: its number, and the character if it is one.)
                    Base::Char if (0x20..0x7f).contains(&v) => write!(&mut out, "{} '{}'", v, v as u8 as char),
                    Base::Char => write!(&mut out, "{}", sign_extend(v, bits.max(1))),
                    Base::Float if size == 4 => write!(&mut out, "{}", f32::from_bits(v as u32)),
                    Base::Float => write!(&mut out, "{}", f64::from_bits(v)),
                    // Unsigned: as num, but flags (arch::FLAG_TYPES: fmode_t,
                    // gfp_t, ...) in hex and file modes in octal.
                    _ if v > 7 => {
                        let name = self.name(ty);
                        let flags = crate::arch::FLAG_TYPES.iter().any(|&f| name == f || f.starts_with('*') && name.ends_with(&f[1..]));
                        match () {
                            _ if v > 9 && flags => write!(&mut out, "{v:#x}"),
                            _ if crate::arch::OCTAL_TYPES.contains(&name.as_str()) => write!(&mut out, "0{v:o}"),
                            _ => write!(&mut out, "{}", num(v)),
                        }
                    }
                    _ => write!(&mut out, "{}", num(v)),
                };
            }
            Kind::Enum(values) => {
                // (An enumerator may be negative: as the value's bits, or signed.)
                let bits = if bit_size > 0 { bit_size } else { t.size.min(8) * 8 };
                let u = self.read_int(mem, addr, t.size.min(8), bit_off, bit_size);
                let v = if bits > 0 { sign_extend(u, bits) } else { u as i64 };
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
                out.push_str(&quote(&c_str(mem, addr, (*count).min(256))));
            }
            Kind::Array { .. } | Kind::Struct(_) | Kind::Union(_) => out.push_str("{...}"),
            Kind::Func => out.push_str("<function>"),
            Kind::Void | Kind::Typedef(_) => out.push_str("<void>"),
        }
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

/// Above this, a number as the page shows one is in hex (an address, a
/// flag word).
const HEX_ABOVE: u64 = 0xffff;

/// A number as the page shows one: in hex past HEX_ABOVE.
pub(crate) fn num(v: u64) -> String {
    if v > HEX_ABOVE { alloc::format!("{v:#x}") } else { v.to_string() }
}

/// The C string at `addr`, to its NUL, of at most `max` bytes.
pub(crate) fn c_str(mem: &dyn Memory, addr: u32, max: u32) -> Vec<u8> {
    let mut buf = alloc::vec![0u8; max as usize];
    mem.read(addr, &mut buf);
    buf.truncate(buf.iter().position(|&c| c == 0).unwrap_or(buf.len()));
    buf
}

/// Bytes as a C string literal: text as text, the rest escaped (\n, \t,
/// \", \\, \0, \xHH); past 64, "…".
pub(crate) fn quote(b: &[u8]) -> String {
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

/// `v`'s low `bits` (1 to 64), a signed number.
fn sign_extend(v: u64, bits: u32) -> i64 {
    (v << (64 - bits)) as i64 >> (64 - bits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::tests::TYPES;

    #[test]
    fn reads_tasks() {
        let t = Types::parse("T\t1\tbase\tint\t4\ts\nT\t2\tbase\tchar\t1\tc\n\
            T\t3\tarray\t\t16\t2\t16\nT\t4\tstruct\ttask_struct\t20\nM\t4\tpid\t0\t0\t1\nM\t4\tcomm\t32\t0\t3\n");
        let mut m = alloc::vec![0u8; 0x200];
        m[0x100..0x104].copy_from_slice(&7u32.to_le_bytes());
        m[0x104..0x108].copy_from_slice(b"ping");
        assert_eq!(t.read_task(&m, 0x100), Some(TaskId { pid: 7, comm: "ping".into() }));
    }
    #[test]
    fn prints_values() {
        let t = Types::parse(TYPES);
        let mut m = alloc::vec![0u8; 32];
        m[0..4].copy_from_slice(&(-3i32).to_le_bytes());
        m[4..8].copy_from_slice(b"init");
        m[12..16].copy_from_slice(&0x40u32.to_le_bytes());
        m[16] = 0b0101; // flags: 3 bits = 5
        let mem = m;
        let at = |addr, ty, off, size| t.print(&mem, addr, ty, off, size);
        assert_eq!((at(0, 1, 0, 0), at(4, 3, 0, 0), at(12, 5, 0, 0), at(16, 7, 0, 3), at(0, 6, 0, 0)),
                   ("-3".into(), "\"init\"".into(), "0x40".into(), "5".into(), "{...}".into()));
    }
}
