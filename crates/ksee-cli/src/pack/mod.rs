//! Package the raw Kbuild kernel for the page (called by `ksee build`): a
//! rewritten image with packed metadata and no DWARF, plus the kernel sources
//! referenced by its probes.

mod dwarf;
mod module;

use std::collections::BTreeSet;
use std::path::{Component, Path};

use ksee_core::arch::SYSCALL_WRAPPER;

/// The syscall table (arch/wasm/kernel/syscall.c): an entry per number,
/// the function it calls, SYSCALL_WRAPPER's `__wasm_sys_NAME`, NULL for one
/// this kernel does not build.
const SYSCALL_TABLE: &str = "sys_call_table";

/// A function's id, as the plugin's probes have it: the 32-bit FNV-1a hash
/// of its name (of "file:name" for one local to its file).
pub fn id(name: &str) -> u32 {
    name.bytes().fold(2166136261u32, |h, c| (h ^ c as u32).wrapping_mul(16777619))
}

/// The columns after the first of each of `text`'s lines of kind `kind`
/// (tab-separated: ksee.probes', ksee.types').
fn rows<'a>(text: &'a str, kind: &'a str) -> impl Iterator<Item = Vec<&'a str>> {
    text.lines().filter_map(move |l| l.strip_prefix(kind)?.strip_prefix('\t').map(|r| r.split('\t').collect()))
}

/// Column `k` (from 1) of `text`'s lines of kind `kind`.
fn column<'a>(text: &'a str, kind: &'a str, k: usize) -> impl Iterator<Item = &'a str> {
    rows(text, kind).filter_map(move |c| c.get(k - 1).copied())
}

/// Write the packed image and its referenced sources to their final paths.
pub fn kernel(kernel: &Path, image_out: &Path, sources_out: &Path) {
    let image = crate::read(&kernel.join("vmlinux"));
    let mut types = crate::or_fail(dwarf::types(&image), "DWARF");
    let probes = String::from_utf8_lossy(&module::custom_section(&image, "ksee.probes")).into_owned();
    for (name, value) in constants(&kernel.join("usr/include")) {
        types += &format!("C\t{name}\t{value}\n");
    }
    let calls = syscalls(&image, &types);
    for (nr, name) in &calls {
        types += &format!("S\t{nr}\t{name}\n");
    }
    let packed = rewrite(&image, &types);
    if let Some(parent) = image_out.parent() {
        crate::or_fail(std::fs::create_dir_all(parent), parent.display());
    }
    crate::or_fail(std::fs::write(image_out, &packed), image_out.display());

    // The sources, for the page's source view.
    // (The functions the trace has whose parameters DWARF gave, by id: the
    // two agree on what a function is.)
    let functions: BTreeSet<&str> = column(&probes, "F", 1).collect();
    let known = column(&types, "P", 1).collect::<BTreeSet<_>>().intersection(&functions).count();
    let sources: Vec<String> = column(&probes, "P", 2).map(String::from).collect();
    let (files, bytes) = publish_sources(kernel, &sources, sources_out);
    eprintln!("{}: {} types, {} syscalls, parameters for {known} of {} functions, {} -> {} KiB; {files} sources, {} KiB",
              image_out.display(), column(&types, "T", 1).count(), calls.len(), functions.len(),
              image.len() >> 10, packed.len() >> 10, bytes >> 10);
}

fn source_path(file: &str) -> Option<&Path> {
    let path = Path::new(file);
    (!file.is_empty() && path.components().all(|c| matches!(c, Component::Normal(_)))).then_some(path)
}

fn publish_sources(kernel: &Path, files: &[String], out: &Path) -> (usize, usize) {
    let _ = std::fs::remove_dir_all(out);
    crate::or_fail(std::fs::create_dir_all(out), out.display());

    let mut paths: Vec<&Path> = files.iter().filter_map(|file| source_path(file)).collect();
    paths.sort_unstable();
    paths.dedup();
    let (mut copied, mut bytes) = (0, 0);
    for path in paths {
        let Ok(data) = std::fs::read(kernel.join(path)) else { continue };
        let to = out.join(path);
        crate::or_fail(std::fs::create_dir_all(to.parent().unwrap()).and_then(|_| std::fs::write(&to, &data)), to.display());
        copied += 1;
        bytes += data.len();
    }
    (copied, bytes)
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
            if let Some(v) = evaluate(value, &known) {
                known.push((name.to_string(), v));
                if prefixes.iter().any(|p| name.starts_with(p)) && !name.starts_with("__") {
                    out.push((name.to_string(), v));
                }
            }
        }
    }
    out
}

/// A `#define`'s value: numbers (C's: octal, hex), the constants `known`,
/// parentheses, unary `-` and `~`, and `| & << >> + -` (C's precedence).
fn evaluate(text: &str, known: &[(String, i64)]) -> Option<i64> {
    // (Tokens: numbers and names whole, else one character, or a shift's two.)
    let mut toks: Vec<&str> = Vec::new();
    let mut rest = text.trim();
    while !rest.is_empty() {
        let n = if rest.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_') {
            rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(rest.len())
        } else if rest.starts_with("<<") || rest.starts_with(">>") { 2 } else { 1 };
        toks.push(&rest[..n]);
        rest = rest[n..].trim_start();
    }
    const LEVELS: [&[&str]; 4] = [&["|"], &["&"], &["<<", ">>"], &["+", "-"]];
    fn binary(t: &[&str], at: &mut usize, level: usize, known: &[(String, i64)]) -> Option<i64> {
        let Some(ops) = LEVELS.get(level) else { return unary(t, at, known) };
        let mut v = binary(t, at, level + 1, known)?;
        while let Some(&op) = t.get(*at).filter(|op| ops.contains(op)) {
            *at += 1;
            let r = binary(t, at, level + 1, known)?;
            v = match op { "|" => v | r, "&" => v & r, "<<" => v << r, ">>" => v >> r, "+" => v + r, _ => v - r };
        }
        Some(v)
    }
    fn unary(t: &[&str], at: &mut usize, known: &[(String, i64)]) -> Option<i64> {
        let tok = *t.get(*at)?;
        *at += 1;
        match tok {
            "-" => Some(-unary(t, at, known)?),
            "~" => Some(!unary(t, at, known)?),
            "(" => {
                let v = binary(t, at, 0, known)?;
                (t.get(*at) == Some(&")")).then(|| { *at += 1; v })
            }
            _ if tok.starts_with(|c: char| c.is_ascii_digit()) => {
                let digits = tok.trim_end_matches(['u', 'U', 'l', 'L']);
                match digits.strip_prefix("0x").or(digits.strip_prefix("0X")) {
                    Some(hex) => i64::from_str_radix(hex, 16).ok(),
                    None if digits.len() > 1 && digits.starts_with('0') => i64::from_str_radix(&digits[1..], 8).ok(),
                    None => digits.parse().ok(),
                }
            }
            _ => known.iter().find(|(k, _)| k == tok).map(|&(_, v)| v),
        }
    }
    let mut at = 0;
    let v = binary(&toks, &mut at, 0, known)?;
    (at == toks.len()).then_some(v)
}

/// The syscalls this kernel builds, as its syscall table (SYSCALL_TABLE,
/// its G line in `types`, its size its type's T line's) is in the image:
/// each entry the function a number calls, SYSCALL_WRAPPER and its name,
/// by its index in the function table; NULL for a syscall it does not
/// build: (number, NAME).
fn syscalls(image: &[u8], types: &str) -> Vec<(u32, String)> {
    let Some((addr, ty)) = rows(types, "G").find(|c| c.first() == Some(&SYSCALL_TABLE))
        .and_then(|c| Some((c.get(1)?.parse::<u32>().ok()?, *c.get(2)?))) else { return Vec::new() };
    let size = rows(types, "T").find(|c| c.first() == Some(&ty)).and_then(|c| c.get(3)?.parse().ok()).unwrap_or(0);
    let entries = module::memory(image, addr, size);
    let (table, names) = (module::table(image), module::function_names(image));
    entries.chunks(4).enumerate().filter_map(|(nr, e)| {
        let f = table.get(&u32::from_le_bytes(e.try_into().ok()?))?;
        Some((nr as u32, names.get(f)?.strip_prefix(SYSCALL_WRAPPER)?.to_string()))
    }).collect()
}

/// `image` without DWARF and with `types` as its ksee.types section.
fn rewrite(image: &[u8], types: &str) -> Vec<u8> {
    let mut out = image[..8].to_vec();
    for (id, name, raw) in module::sections(image) {
        if !(id == 0 && name.starts_with(".debug_")) {
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
mod source_tests {
    use super::*;

    #[test]
    fn source_paths_are_relative_and_normal() {
        for path in ["kernel/fork.c", "name..c"] {
            assert_eq!(source_path(path), Some(Path::new(path)));
        }
        for path in ["", "/kernel/fork.c", "../fork.c", "kernel/../fork.c", "."] {
            assert!(source_path(path).is_none(), "accepted {path}");
        }
    }

    #[test]
    fn sources_are_sorted_deduplicated_and_replaced() {
        let root = crate::Scratch::new("pack-sources");
        let kernel = root.0.join("kernel");
        let out = root.0.join("out");
        std::fs::create_dir_all(kernel.join("a")).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(kernel.join("a/one.c"), b"one").unwrap();
        std::fs::write(kernel.join("two.c"), b"two").unwrap();
        std::fs::write(out.join("old.c"), b"old").unwrap();
        let files = ["two.c", "a/one.c", "two.c", "missing.c", "../outside"]
            .map(str::to_string);

        assert_eq!(publish_sources(&kernel, &files, &out), (2, 6));
        assert_eq!(std::fs::read(out.join("a/one.c")).unwrap(), b"one");
        assert_eq!(std::fs::read(out.join("two.c")).unwrap(), b"two");
        assert!(!out.join("old.c").exists());
    }

    #[test]
    fn an_empty_source_set_still_replaces_the_directory() {
        let root = crate::Scratch::new("pack-empty");
        let out = root.0.join("out");
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("old.c"), b"old").unwrap();

        assert_eq!(publish_sources(&root.0, &[], &out), (0, 0));
        assert!(out.is_dir());
        assert!(std::fs::read_dir(&out).unwrap().next().is_none());
    }
}

#[cfg(test)]
mod constants_tests {
    use super::*;
    use crate::Scratch;
    use std::collections::HashMap;

    /// `include/` with two headers: numbers in every shape, and the ones to skip.
    fn headers() -> Scratch {
        let scratch = Scratch::new("pack-constants");
        let root = &scratch.0;
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
        scratch
    }

    #[test]
    fn constants_from_uapi_defines() {
        let root = headers();
        let got: HashMap<String, i64> = constants(&root.0).into_iter().collect();
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
        [&b"\0asm\x01\0\0\0"[..], &section(10, &[1, 2, 3]), &custom(".debug_info", b"DWARF"), &custom("name", b"kept")].concat()
    }

    #[test]
    fn rewrite_drops_dwarf_and_adds_types() {
        let out = rewrite(&image(), "T\t1\tbase\tint\t4\ts\n");
        assert_eq!(&out[..8], b"\0asm\x01\0\0\0");
        assert!(module::custom_section(&out, ".debug_info").is_empty());
        assert_eq!(module::custom_section(&out, "ksee.types"), b"T\t1\tbase\tint\t4\ts\n");
        assert_eq!(module::custom_section(&out, "name"), b"kept");
        let kept: Vec<_> = module::sections(&out).filter(|s| s.0 == 10).collect();
        assert_eq!(kept.len(), 1);
        assert_eq!(&kept[0].2[2..], &[1, 2, 3]);
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
        assert_eq!(syscalls(&syscall_image(), "T\t1\tbase\tint\t8\tu\nG\tsys_call_table\t256\t1\n"), [(0, "read".to_string())]);
    }

    #[test]
    fn ids_are_the_plugins() {
        // (From a kernel's ksee.probes section.)
        assert_eq!(id("try_to_wake_up"), 2218239081);
    }

    #[test]
    fn syscalls_without_a_table_are_none() {
        assert!(syscalls(&syscall_image(), "T\t1\tbase\tint\t8\tu\n").is_empty());
    }
}
