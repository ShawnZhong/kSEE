//! What the host reads of a wasm module itself: its custom sections (the
//! kernel's probe tables; a program's dylink.0), where it is, in memory or
//! not, with no copy of it.

use alloc::string::String;
use alloc::vec::Vec;

use crate::Memory;

/// An unsigned LEB128 number at `*at` (moved past it).
fn leb(m: &dyn Memory, at: &mut u32) -> Option<u32> {
    let mut v = 0u32;
    for shift in (0..35).step_by(7) {
        let x = m.read_u8(*at);
        *at += 1;
        v |= ((x & 0x7f) as u32).checked_shl(shift)?;
        if x < 0x80 {
            return Some(v);
        }
    }
    None
}

/// The custom sections of the module at `base`, `len` bytes: each its name
/// and where its contents are (their address and length).
pub fn custom_sections(m: &dyn Memory, base: u32, len: u32) -> Vec<(String, u32, u32)> {
    let mut out = Vec::new();
    let end = base.saturating_add(len);
    // (Past the magic and the version.)
    let mut at = base + 8;
    while at < end {
        let id = m.read_u8(at);
        at += 1;
        let Some(size) = leb(m, &mut at) else { break };
        let next = at.saturating_add(size);
        if id == 0 {
            let mut p = at;
            let Some(n) = leb(m, &mut p) else { break };
            let mut name = alloc::vec![0; n as usize];
            m.read(p, &mut name);
            p += n;
            out.push((String::from_utf8_lossy(&name).into_owned(), p, next.saturating_sub(p)));
        }
        at = next;
    }
    out
}

/// The contents of the module's custom sections named `name`, one after
/// another.
pub fn section(m: &dyn Memory, base: u32, len: u32, name: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for (_, at, n) in custom_sections(m, base, len).into_iter().filter(|s| s.0 == name) {
        let k = out.len();
        out.resize(k + n as usize, 0);
        m.read(at, &mut out[k..]);
    }
    out
}

/// A program's dylink.0 memory info (the tool conventions' dynamic
/// linking): the bytes its data needs, and its table entries.
pub fn dylink(m: &dyn Memory, base: u32, len: u32) -> Option<(u32, u32)> {
    let (_, mut at, _) = custom_sections(m, base, len).into_iter().find(|s| s.0 == "dylink.0")?;
    // (Subsection 1, WASM_DYLINK_MEM_INFO, comes first.)
    let (kind, _len) = (leb(m, &mut at)?, leb(m, &mut at)?);
    if kind != 1 {
        return None;
    }
    let (mem_size, _align, table_size) = (leb(m, &mut at)?, leb(m, &mut at)?, leb(m, &mut at)?);
    Some((mem_size, table_size))
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

    /// A module: its header, then each section (id, contents).
    fn module(sections: &[(u8, Vec<u8>)]) -> Vec<u8> {
        let mut b = b"\0asm\x01\0\0\0".to_vec();
        for (id, body) in sections {
            b.push(*id);
            // (A two-byte LEB128 size, as a writer may pad it.)
            b.extend([(body.len() as u8 & 0x7f) | 0x80, (body.len() >> 7) as u8]);
            b.extend(body);
        }
        b
    }

    fn custom(name: &str, body: &[u8]) -> (u8, Vec<u8>) {
        let mut b = alloc::vec![name.len() as u8];
        b.extend(name.as_bytes());
        b.extend(body);
        (0, b)
    }

    #[test]
    fn custom_sections_by_name_past_the_others() {
        let b = module(&[custom("ksee.types", b"T\t1"), (1, alloc::vec![0x60, 0, 0]), custom("ksee.types", b"\nM"), custom("x", b"")]);
        let m = Mem(b.clone());
        assert_eq!(section(&m, 0, b.len() as u32, "ksee.types"), b"T\t1\nM");
        assert_eq!(custom_sections(&m, 0, b.len() as u32).len(), 3);
    }

    #[test]
    fn dylink_mem_info() {
        // (mem_size 300: a two-byte LEB128.)
        let b = module(&[custom("dylink.0", &[1, 5, 0xac, 0x02, 2, 7, 0])]);
        // (At an address, as a program in kernel memory is.)
        let mut at = alloc::vec![0u8; 16];
        at.extend(&b);
        assert_eq!(dylink(&Mem(at), 16, b.len() as u32), Some((300, 7)));
    }
}
