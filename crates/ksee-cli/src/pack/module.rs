//! Reading a wasm module: its custom sections (the kernel's
//! self-description), and what its linker put in memory and in its
//! function table (the kernel's syscall table: see ksee pack).

use std::collections::BTreeMap;

/// An unsigned LEB128 value at `*pos` in `buf`, `*pos` past it.
pub fn uleb(buf: &[u8], pos: &mut usize) -> Option<u32> {
    let (mut v, mut shift) = (0u32, 0);
    loop {
        let b = *buf.get(*pos)?;
        *pos += 1;
        if shift >= 32 {
            return None; // corrupt: longer than any u32
        }
        v |= ((b & 0x7f) as u32) << shift;
        if b & 0x80 == 0 {
            return Some(v);
        }
        shift += 7;
    }
}

/// `v` as unsigned LEB128, appended to `out`.
pub fn leb(out: &mut Vec<u8>, mut v: u32) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        out.push(if v == 0 { b } else { b | 0x80 });
        if v == 0 {
            break;
        }
    }
}

fn sleb(buf: &[u8], pos: &mut usize) -> Option<i64> {
    let (mut v, mut shift) = (0i64, 0);
    loop {
        let b = *buf.get(*pos)?;
        *pos += 1;
        if shift >= 64 {
            return None; // corrupt: longer than any i64
        }
        v |= ((b & 0x7f) as i64) << shift;
        shift += 7;
        if b & 0x80 == 0 {
            return Some(if shift < 64 && b & 0x40 != 0 { v | -1 << shift } else { v });
        }
    }
}

/// A constant expression's value, `i32.const n` (an active segment's
/// offset).
fn offset(buf: &[u8], pos: &mut usize) -> Option<u32> {
    (*buf.get(*pos)? == 0x41).then_some(())?;
    *pos += 1;
    let v = sleb(buf, pos)? as u32;
    (*buf.get(*pos)? == 0x0b).then_some(())?;
    *pos += 1;
    Some(v)
}

/// The body of the (first) section with id `id`.
fn body(module: &[u8], id: u8) -> Option<&[u8]> {
    let (_, _, raw) = sections(module).find(|s| s.0 == id)?;
    let mut p = 1;
    uleb(raw, &mut p)?;
    Some(&raw[p..])
}

/// `len` bytes of memory at `addr` as the module's active data segments
/// set it (zero where none does).
pub fn memory(module: &[u8], addr: u32, len: u32) -> Vec<u8> {
    let mut out = vec![0u8; len as usize];
    let Some(b) = body(module, 11) else { return out };
    let mut p = 0;
    let n = uleb(b, &mut p).unwrap_or(0);
    for _ in 0..n {
        let Some(flags) = uleb(b, &mut p) else { break };
        let at = match flags {
            0 => offset(b, &mut p),
            2 => uleb(b, &mut p).and_then(|_| offset(b, &mut p)),
            _ => None, // passive
        };
        let Some(size) = uleb(b, &mut p) else { break };
        let bytes = &b[p.min(b.len())..(p + size as usize).min(b.len())];
        p += size as usize;
        let Some(at) = at else { continue };
        for (k, &byte) in bytes.iter().enumerate() {
            if let Some(i) = (at as usize + k).checked_sub(addr as usize).filter(|&i| i < out.len()) {
                out[i] = byte;
            }
        }
    }
    out
}

/// The function table as the module's active element segments set it:
/// table index -> function index.
pub fn table(module: &[u8]) -> BTreeMap<u32, u32> {
    let mut out = BTreeMap::new();
    let Some(b) = body(module, 9) else { return out };
    let mut p = 0;
    let n = uleb(b, &mut p).unwrap_or(0);
    for _ in 0..n {
        // (Only the plain kind, flags 0, a linker's: offset, then function indices.)
        if uleb(b, &mut p) != Some(0) {
            break;
        }
        let (Some(at), Some(count)) = (offset(b, &mut p), uleb(b, &mut p)) else { break };
        for k in 0..count {
            let Some(f) = uleb(b, &mut p) else { break };
            out.insert(at + k, f);
        }
    }
    out
}

/// The functions' names (the name section's): function index -> name.
pub fn function_names(module: &[u8]) -> BTreeMap<u32, String> {
    let mut out = BTreeMap::new();
    let b = custom_section(module, "name");
    let mut p = 0;
    while p < b.len() {
        let (Some(kind), Some(size)) = (b.get(p).copied(), { p += 1; uleb(&b, &mut p) }) else { break };
        let end = p + size as usize;
        if kind == 1 {
            let n = uleb(&b, &mut p).unwrap_or(0);
            for _ in 0..n {
                let (Some(f), Some(len)) = (uleb(&b, &mut p), uleb(&b, &mut p)) else { break };
                let name = b.get(p..p + len as usize).map(|s| String::from_utf8_lossy(s).into_owned()).unwrap_or_default();
                p += len as usize;
                out.insert(f, name);
            }
        }
        p = end;
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leb_round_trips() {
        for v in [0, 1, 127, 128, 300, u32::MAX] {
            let mut out = Vec::new();
            leb(&mut out, v);
            let mut p = 0;
            assert_eq!(uleb(&out, &mut p), Some(v));
            assert_eq!(p, out.len());
        }
        // (Corrupt: longer than any u32, never terminates: None.)
        let mut p = 0;
        assert_eq!(uleb(&[0x80; 6], &mut p), None);
    }

    #[test]
    fn a_modules_memory_table_and_names() {
        // Table 1.. = functions 3, 5; memory 8.. = 1, 0, 0, 0; function 3 is "read".
        let m: Vec<u8> = [&b"\0asm\x01\0\0\0"[..],
            &[9, 8, 1, 0, 0x41, 1, 0x0b, 2, 3, 5],
            &[11, 10, 1, 0, 0x41, 8, 0x0b, 4, 1, 0, 0, 0],
            &[0, 14, 4], b"name", &[1, 7, 1, 3, 4], b"read"].concat();
        assert_eq!(memory(&m, 6, 6), [0, 0, 1, 0, 0, 0]);
        assert_eq!(table(&m).into_iter().collect::<Vec<_>>(), [(1, 3), (2, 5)]);
        assert_eq!(function_names(&m).get(&3).map(String::as_str), Some("read"));
    }
}
