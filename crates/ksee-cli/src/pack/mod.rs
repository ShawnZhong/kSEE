//! Package the raw Kbuild kernel for the page (called by `ksee build`): its
//! tables (the plugin's, and its DWARF's), plus the kernel sources
//! referenced by its probes. (The image itself, without them, is the
//! Makefile's: llvm-objcopy's.)

mod dwarf;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

use ksee_core::arch::SYSCALL_WRAPPER;

/// A function's id, as the plugin's probes have it: the 32-bit FNV-1a hash
/// of its name (of "file:name" for one local to its file).
pub fn id(name: &str) -> u32 {
    name.bytes().fold(2166136261u32, |h, c| (h ^ c as u32).wrapping_mul(16777619))
}

/// The columns after the first of each of `text`'s lines of kind `kind`
/// (tab-separated: the ksee section's, the types').
fn rows<'a>(text: &'a str, kind: &'a str) -> impl Iterator<Item = Vec<&'a str>> {
    text.lines().filter_map(move |l| l.strip_prefix(kind)?.strip_prefix('\t').map(|r| r.split('\t').collect()))
}

/// Column `k` (from 1) of `text`'s lines of kind `kind`.
fn column<'a>(text: &'a str, kind: &'a str, k: usize) -> impl Iterator<Item = &'a str> {
    rows(text, kind).filter_map(move |c| c.get(k - 1).copied())
}

/// The plugin's ksee section, of the kernel's description's sections.
fn code(sections: &BTreeMap<&str, &[u8]>) -> String {
    String::from_utf8_lossy(sections.get("ksee").copied().unwrap_or(&[])).into_owned()
}

/// Write the kernel's tables (parsed here, once, not by the page) to
/// `out`: of its description `debug` and its UAPI headers `headers`.
pub fn tables(debug: &Path, headers: &Path, out: &Path) {
    let image = crate::read(debug);
    let sections = crate::or_fail(custom_sections(&image), debug.display());
    let code = code(&sections);
    let mut types = crate::or_fail(dwarf::types(&sections), "DWARF");
    for (name, value) in constants(headers) {
        types += &format!("C\t{name}\t{value}\n");
    }
    let calls = syscalls(&code);
    for (nr, name) in &calls {
        types += &format!("S\t{nr}\t{name}\n");
    }
    let tables = ksee_core::tables::Tables::parse(&code, &types).to_bytes();
    crate::or_fail(std::fs::write(out, &tables), out.display());
    // (The functions the trace has whose parameters DWARF gave, by id: the
    // two agree on what a function is.)
    let functions: BTreeSet<&str> = column(&code, "F", 1).collect();
    let known = column(&types, "P", 1).collect::<BTreeSet<_>>().intersection(&functions).count();
    eprintln!("{}: {} types, {} syscalls, parameters for {known} of {} functions, {} KiB",
              out.display(), column(&types, "T", 1).count(), calls.len(), functions.len(), tables.len() >> 10);
}

/// Copy the sources the kernel's description `debug` names (its P lines),
/// of its tree `kernel`, to `out`, for the page's source view.
pub fn sources(debug: &Path, kernel: &Path, out: &Path) {
    let image = crate::read(debug);
    let code = code(&crate::or_fail(custom_sections(&image), debug.display()));
    let sources: Vec<String> = column(&code, "P", 2).map(String::from).collect();
    let (files, bytes) = publish_sources(kernel, &sources, out);
    eprintln!("{}: {files} sources, {} KiB", out.display(), bytes >> 10);
}

/// The custom sections of a wasm module (vmlinux.debug's: the kernel's
/// DWARF, the plugin's ksee section), by name, in place.
fn custom_sections(bytes: &[u8]) -> Result<BTreeMap<&str, &[u8]>, wasmparser::BinaryReaderError> {
    let mut out = BTreeMap::new();
    for payload in wasmparser::Parser::new(0).parse_all(bytes) {
        if let wasmparser::Payload::CustomSection(c) = payload? {
            out.insert(c.name(), c.data());
        }
    }
    Ok(out)
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

/// A `#define`'s value: a C expression of numbers and the constants
/// `known` (ksee_core's, as a program's are).
fn evaluate(text: &str, known: &[(String, i64)]) -> Option<i64> {
    let named = |n: &str| known.iter().find(|(k, _)| k == n).map(|&(_, v)| v);
    ksee_core::program::value(&ksee_core::program::expression(text).ok()?, None, &named).ok()
}

/// The syscalls this kernel builds: its syscall table's entries (the
/// plugin's S lines: number, function id) whose function it has (an F
/// line: SYSCALL_WRAPPER and the name): (number, NAME).
fn syscalls(code: &str) -> Vec<(u32, String)> {
    let names: BTreeMap<&str, &str> = rows(code, "F").filter_map(|c| Some((*c.first()?, *c.get(1)?))).collect();
    let calls: BTreeSet<(u32, String)> = rows(code, "S").filter_map(|c| {
        Some((c.first()?.parse().ok()?, names.get(c.get(1)?)?.strip_prefix(SYSCALL_WRAPPER)?.to_string()))
    }).collect();
    calls.into_iter().collect()
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
mod code_tests {
    use super::*;

    #[test]
    fn syscalls_are_the_tables_entries_the_kernel_has() {
        // (0 calls __wasm_sys_read, 1 a function the kernel does not have.)
        let code = "F\t7\t__wasm_sys_read\t0:1\t1\t0\t0\nS\t0\t7\nS\t1\t9\n";
        assert_eq!(syscalls(code), [(0, "read".to_string())]);
    }

    #[test]
    fn ids_are_the_plugins() {
        // (From a kernel's ksee section.)
        assert_eq!(id("try_to_wake_up"), 2218239081);
    }
}
