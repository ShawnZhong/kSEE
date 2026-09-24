//! Reading custom sections of a wasm module (the kernel's self-description).

use alloc::vec::Vec;

fn uleb(buf: &[u8], pos: &mut usize) -> Option<u32> {
    let (mut v, mut shift) = (0u32, 0);
    loop {
        let b = *buf.get(*pos)?;
        *pos += 1;
        v |= ((b & 0x7f) as u32) << shift;
        if b & 0x80 == 0 {
            return Some(v);
        }
        shift += 7;
    }
}

/// The contents of every custom section called `name`, concatenated.
pub fn custom_section(module: &[u8], name: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut pos = 8;
    while pos < module.len() {
        let id = module[pos];
        pos += 1;
        let Some(size) = uleb(module, &mut pos) else { break };
        let end = pos + size as usize;
        if id == 0 {
            let mut p = pos;
            if let Some(n) = uleb(module, &mut p) {
                let n = n as usize;
                if module.get(p..p + n) == Some(name.as_bytes()) {
                    out.extend_from_slice(&module[p + n..end.min(module.len())]);
                }
            }
        }
        pos = end;
    }
    out
}
