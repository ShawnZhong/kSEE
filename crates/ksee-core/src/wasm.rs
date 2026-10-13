//! What the host reads of a wasm module itself: a program's dylink.0,
//! where it is in memory, with no copy of it.

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

/// A program's dylink.0 memory info (the tool conventions' dynamic
/// linking: its first section), of the module at `base`, `len` bytes: the
/// bytes its data needs, and its table entries.
pub fn dylink(m: &dyn Memory, base: u32, len: u32) -> Option<(u32, u32)> {
    const NAME: &[u8] = b"dylink.0";
    // (Past the magic and the version: a custom section, so named.)
    let mut at = base + 8;
    if len < 8 || m.read_u8(at) != 0 {
        return None;
    }
    at += 1;
    let _size = leb(m, &mut at)?;
    let mut name = [0; NAME.len()];
    if leb(m, &mut at)? as usize != NAME.len() || { m.read(at, &mut name); name != NAME } {
        return None;
    }
    at += NAME.len() as u32;
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
    use alloc::vec::Vec;


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
    fn dylink_mem_info() {
        // (mem_size 300: a two-byte LEB128.)
        let b = module(&[custom("dylink.0", &[1, 5, 0xac, 0x02, 2, 7, 0])]);
        // (At an address, as a program in kernel memory is.)
        let mut at = alloc::vec![0u8; 16];
        at.extend(&b);
        assert_eq!(dylink(&at, 16, b.len() as u32), Some((300, 7)));
    }
}
