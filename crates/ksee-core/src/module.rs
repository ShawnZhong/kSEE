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

/// Every section: (id, custom section name or "", the whole section's bytes
/// including its header).
pub fn sections(module: &[u8]) -> impl Iterator<Item = (u8, &str, &[u8])> {
    let mut pos = 8;
    core::iter::from_fn(move || {
        if pos >= module.len() {
            return None;
        }
        let start = pos;
        let id = module[pos];
        pos += 1;
        let size = uleb(module, &mut pos)? as usize;
        let body = pos;
        pos = (body + size).min(module.len());
        let mut name = "";
        if id == 0 {
            let mut p = body;
            let n = uleb(module, &mut p)? as usize;
            name = core::str::from_utf8(module.get(p..p + n)?).unwrap_or("");
        }
        Some((id, name, &module[start..pos]))
    })
}

/// Custom sections: (name, contents).
pub fn custom_sections(module: &[u8]) -> impl Iterator<Item = (&str, &[u8])> {
    sections(module).filter(|s| s.0 == 0).map(|(_, name, raw)| {
        let mut p = 1;
        let _ = uleb(raw, &mut p); // section size
        let n = uleb(raw, &mut p).unwrap_or(0) as usize;
        (name, &raw[p + n..])
    })
}

/// The contents of every custom section called `name`, concatenated.
pub fn custom_section(module: &[u8], name: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for (n, data) in custom_sections(module) {
        if n == name {
            out.extend_from_slice(data);
        }
    }
    out
}
