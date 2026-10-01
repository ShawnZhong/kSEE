//! The wasm image's parts the page needs: the syscalls its syscall table
//! names, and the image rewritten without DWARF and with the type table as
//! its ksee.types section.

use ksee_core::module;

/// The syscalls this kernel builds, as its syscall table (arch::
/// SYSCALL_TABLE) is in the image: each entry the function a number
/// calls, `__wasm_sys_NAME` (arch::SYSCALL_WRAPPER), by its index in the
/// function table; NULL for a syscall it does not build: (number, NAME).
pub(crate) fn syscalls(image: &[u8], t: &ksee_core::types::Types) -> Vec<(u32, String)> {
    use ksee_core::arch;
    let Some(&(addr, ty)) = t.globals.get(arch::SYSCALL_TABLE) else { return Vec::new() };
    let entries = module::memory(image, addr, t.size_of(ty));
    let (table, names) = (module::table(image), module::function_names(image));
    entries.chunks(4).enumerate().filter_map(|(nr, e)| {
        let f = table.get(&u32::from_le_bytes(e.try_into().ok()?))?;
        Some((nr as u32, names.get(f)?.strip_prefix(arch::SYSCALL_WRAPPER)?.to_string()))
    }).collect()
}

/// `image` without DWARF and with `types` as its ksee.types section.
pub(crate) fn rewrite(image: &[u8], types: &str) -> Vec<u8> {
    let mut out = image[..8].to_vec();
    for (id, name, raw) in module::sections(image) {
        let drop = id == 0 && (name.starts_with(".debug_") || name == "ksee.types");
        if !drop {
            out.extend_from_slice(raw);
        }
    }
    let mut payload = Vec::new();
    module::leb(&mut payload, "ksee.types".len() as u32);
    payload.extend_from_slice(b"ksee.types");
    payload.extend_from_slice(types.as_bytes());
    out.push(0);
    module::leb(&mut out, payload.len() as u32);
    out.extend_from_slice(&payload);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(id: u8, payload: &[u8]) -> Vec<u8> {
        let mut s = vec![id];
        module::leb(&mut s, payload.len() as u32);
        s.extend_from_slice(payload);
        s
    }

    fn custom(name: &str, contents: &[u8]) -> Vec<u8> {
        let mut payload = Vec::new();
        module::leb(&mut payload, name.len() as u32);
        payload.extend_from_slice(name.as_bytes());
        payload.extend_from_slice(contents);
        section(0, &payload)
    }

    fn image() -> Vec<u8> {
        [&b"\0asm\x01\0\0\0"[..], &section(10, &[1, 2, 3]), &custom(".debug_info", b"DWARF"),
            &custom("ksee.types", b"old"), &custom("name", b"kept")].concat()
    }

    #[test]
    fn rewrite_drops_dwarf_and_replaces_types() {
        let out = rewrite(&image(), "T\t1\tbase\tint\t4\ts\n");
        assert_eq!(&out[..8], b"\0asm\x01\0\0\0");
        assert!(module::custom_section(&out, ".debug_info").is_empty());
        assert_eq!(module::custom_section(&out, "ksee.types"), b"T\t1\tbase\tint\t4\ts\n");
        assert_eq!(module::custom_section(&out, "name"), b"kept");
        let kept: Vec<_> = module::sections(&out).filter(|s| s.0 == 10).collect();
        assert_eq!(kept.len(), 1);
        assert_eq!(&kept[0].2[2..], &[1, 2, 3]);
    }

    #[test]
    fn rewrite_adds_types_where_there_were_none() {
        let bare: Vec<u8> = [&b"\0asm\x01\0\0\0"[..], &section(10, &[9])].concat();
        let out = rewrite(&bare, "T\t1\tbase\tint\t4\ts\n");
        assert_eq!(module::custom_section(&out, "ksee.types"), b"T\t1\tbase\tint\t4\ts\n");
        assert_eq!(module::sections(&out).count(), 2);
    }

    /// An image whose table calls `__wasm_sys_read` at 0 and nothing at 1.
    fn syscall_image() -> Vec<u8> {
        // (The table's two entries, at 0x100: table index 5, then NULL.)
        let mut data = vec![1, 0, 0x41, 0x80, 0x02, 0x0b];
        module::leb(&mut data, 8);
        data.extend_from_slice(&[5, 0, 0, 0, 0, 0, 0, 0]);
        // (Table index 5 is function 7.)
        let mut elem = vec![1, 0, 0x41, 0x05, 0x0b];
        module::leb(&mut elem, 1);
        module::leb(&mut elem, 7);
        // (Function 7 is __wasm_sys_read.)
        let mut names = vec![1];
        let mut sub = Vec::new();
        module::leb(&mut sub, 1);
        module::leb(&mut sub, 7);
        module::leb(&mut sub, "__wasm_sys_read".len() as u32);
        sub.extend_from_slice(b"__wasm_sys_read");
        module::leb(&mut names, sub.len() as u32);
        names.extend_from_slice(&sub);
        [&b"\0asm\x01\0\0\0"[..], &section(11, &data), &section(9, &elem), &custom("name", &names)].concat()
    }

    #[test]
    fn syscalls_names_come_from_the_table() {
        let t = ksee_core::types::Types::parse("T\t1\tbase\tint\t8\tu\nG\tsys_call_table\t256\t1\n");
        assert_eq!(syscalls(&syscall_image(), &t), [(0, "read".to_string())]);
    }

    #[test]
    fn syscalls_without_a_table_are_none() {
        let t = ksee_core::types::Types::parse("T\t1\tbase\tint\t8\tu\n");
        assert!(syscalls(&syscall_image(), &t).is_empty());
    }
}
