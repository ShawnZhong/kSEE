//! Pack the kernel for the page (called by `ksee build`): OUT/vmlinux,
//! with the kernel's types and syscalls from DWARF (see ksee_core::types)
//! and without DWARF; and OUT/src, the sources its probes refer to.

mod dwarf;

use std::path::Path;

use ksee_core::{module, tables::Tables};

/// Write OUT/vmlinux (from KERNEL/vmlinux) and OUT/src (from KERNEL).
pub fn pack(kernel: &Path, out: &Path) {
    let image = crate::read(&kernel.join("vmlinux"));
    let mut types = dwarf::types(&image).unwrap_or_else(|e| crate::fail(&format!("DWARF: {e}")));
    let t = ksee_core::types::Types::parse(&types);
    for (name, value) in constants(&kernel.join("usr/include")) {
        types += &format!("C\t{name}\t{value}\n");
    }
    let calls = syscalls(&image, &t);
    for (nr, name) in &calls {
        types += &format!("S\t{nr}\t{name}\n");
    }
    let packed = rewrite(&image, &types);
    std::fs::create_dir_all(out).unwrap_or_else(|e| crate::fail(&format!("{}: {e}", out.display())));
    std::fs::write(out.join("vmlinux"), &packed).unwrap_or_else(|e| crate::fail(&format!("{}: {e}", out.display())));

    // The sources, for the page's source view.
    let probes = String::from_utf8_lossy(&module::custom_section(&image, "ksee.probes")).into_owned();
    let tables = Tables::parse(&probes, &types);
    // (The functions the trace has whose parameters DWARF gave, by id: the
    // two agree on what a function is.)
    let known = tables.functions.keys().filter(|id| tables.types.params.contains_key(id)).count();
    let src = out.join("src");
    let _ = std::fs::remove_dir_all(&src);
    let (mut files, mut bytes) = (0, 0);
    for file in &tables.files {
        if file.starts_with('/') || file.contains("..") {
            continue; // outside the tree
        }
        if let Ok(data) = std::fs::read(kernel.join(file)) {
            let to = src.join(file);
            let _ = std::fs::create_dir_all(to.parent().unwrap());
            std::fs::write(&to, &data).unwrap_or_else(|e| crate::fail(&format!("{}: {e}", to.display())));
            files += 1;
            bytes += data.len();
        }
    }
    eprintln!("{}: {} types, {} syscalls, parameters for {known} of {} functions, {} -> {} KiB; {files} sources, {} KiB",
              out.display(), types.lines().filter(|l| l.starts_with('T')).count(), calls.len(), tables.functions.len(),
              image.len() >> 10, packed.len() >> 10, bytes >> 10);
}

/// The UAPI headers the page's constants come from, with the prefixes kept.
const CONSTANTS: &[(&str, &[&str])] = &[
    ("asm-generic/fcntl.h", &["O_"]),
    ("linux/fcntl.h", &["AT_"]),
    ("linux/time.h", &["CLOCK_", "TIMER_"]),
    ("asm-generic/errno-base.h", &["E"]),
    ("asm-generic/errno.h", &["E"]),
    ("linux/sched.h", &["CLONE_"]),
    ("linux/futex.h", &["FUTEX_"]),
    ("asm-generic/mman-common.h", &["PROT_", "MAP_"]),
    ("linux/mman.h", &["MAP_"]),
    ("asm-generic/signal.h", &["SIG"]),
    ("linux/wait.h", &["P_", "W"]),
    ("linux/prctl.h", &["PR_"]),
];

/// Constants from the kernel's UAPI headers (under `include`): each
/// `#define NAME VALUE` whose value is a number, or `|`, `<<`, `-` and
/// parentheses of numbers and constants before it.
fn constants(include: &Path) -> Vec<(String, i64)> {
    let mut out: Vec<(String, i64)> = Vec::new();
    // (Every define seen, for the ones after it: O_SYNC is (__O_SYNC|O_DSYNC).)
    let mut known: Vec<(String, i64)> = Vec::new();
    for (file, prefixes) in CONSTANTS {
        let text = String::from_utf8_lossy(&crate::read(&include.join(file))).into_owned();
        for line in text.lines() {
            let Some(rest) = line.trim_start().strip_prefix("#define") else { continue };
            let rest = rest.split("/*").next().unwrap_or("").trim();
            let Some((name, value)) = rest.split_once(char::is_whitespace) else { continue };
            if name.contains('(') || known.iter().any(|(n, _)| n == name) {
                continue;
            }
            if let Ok(v) = ksee_core::program::expression(value).and_then(|e| ksee_core::program::value(&e, None, &|n: &str| known.iter().find(|(k, _)| k == n).map(|&(_, v)| v))) {
                known.push((name.to_string(), v));
                if prefixes.iter().any(|p| name.starts_with(p)) && !name.starts_with("__") {
                    out.push((name.to_string(), v));
                }
            }
        }
    }
    out
}

/// The syscalls this kernel builds, as its syscall table (arch::
/// SYSCALL_TABLE) is in the image: each entry the function a number
/// calls, `__wasm_sys_NAME` (arch::SYSCALL_WRAPPER), by its index in the
/// function table; NULL for a syscall it does not build: (number, NAME).
fn syscalls(image: &[u8], t: &ksee_core::types::Types) -> Vec<(u32, String)> {
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
fn rewrite(image: &[u8], types: &str) -> Vec<u8> {
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
mod constants_tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    /// `include/` with two headers: numbers in every shape, and the ones to skip.
    fn headers() -> PathBuf {
        let root = std::env::temp_dir().join(format!("ksee-pack-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // (Defines used before they are defined resolve: O_SYNC is (__O_SYNC|O_DSYNC).)
        let fcntl = "#define O_ACCMODE 00000003\n#define O_RDONLY 00000000\n#define __O_SYNC 000040000\n#define O_DSYNC 000010000\n\
            #define O_SYNC (__O_SYNC|O_DSYNC)\n#define O_TMPFILE_MASK(x) 1\n#define SEEK_SET 0\n";
        let errno = "#define EPERM 1\n#define ENOENT 2 /* No such file */\n#define EPERM 99\n#define STRANGE (1 << 3)\n";
        std::fs::create_dir_all(root.join("asm-generic")).unwrap();
        std::fs::write(root.join("asm-generic/fcntl.h"), fcntl).unwrap();
        std::fs::write(root.join("asm-generic/errno-base.h"), errno).unwrap();
        for (file, _) in CONSTANTS {
            let p = root.join(file);
            if !p.exists() {
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                std::fs::write(&p, "").unwrap();
            }
        }
        root
    }

    #[test]
    fn constants_from_uapi_defines() {
        let root = headers();
        let got: HashMap<String, i64> = constants(&root).into_iter().collect();
        let _ = std::fs::remove_dir_all(&root);
        // (Octal, a name used before it is defined, a comment, a duplicate kept first.)
        assert_eq!(got.get("O_ACCMODE"), Some(&3));
        assert_eq!(got.get("O_RDONLY"), Some(&0));
        assert_eq!(got.get("O_SYNC"), Some(&0b101000000000000));
        assert_eq!(got.get("EPERM"), Some(&1));
        assert_eq!(got.get("ENOENT"), Some(&2));
        for dropped in ["__O_SYNC", "O_TMPFILE_MASK", "SEEK_SET", "STRANGE"] {
            assert!(!got.contains_key(dropped), "{dropped}");
        }
    }
}

#[cfg(test)]
mod image_tests {
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
