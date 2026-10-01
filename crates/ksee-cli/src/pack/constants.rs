//! The page's constants: each UAPI header's `#define`s with the prefixes
//! kept (flags, errno names), and the enums those headers declare, as DWARF
//! has them.

use std::path::Path;

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
    ("linux/io_uring.h", &["IORING_", "IOSQE_"]),
];

/// The values of the enums those headers declare (IORING_OP_READ), as
/// DWARF has them, with the prefixes kept.
pub(crate) fn enumerators(t: &ksee_core::types::Types) -> Vec<(String, i64)> {
    t.types.iter().flat_map(|ty| {
        let ksee_core::types::Kind::Enum(values) = &ty.kind else { return Vec::new() };
        let header = t.declared.get(&ty.name).and_then(|(file, _)| CONSTANTS.iter().find(|(h, _)| file.ends_with(&format!("uapi/{h}"))));
        let Some((_, prefixes)) = header else { return Vec::new() };
        values.iter().filter(|(n, _)| prefixes.iter().any(|p| n.starts_with(p))).cloned().collect()
    }).collect()
}

/// Constants from the kernel's UAPI headers (under `include`: after `make
/// headers`), for the page (`C name value`: flags, errno names): each
/// `#define NAME VALUE` whose value is a number, or `|`, `<<`, `-` and
/// parentheses of numbers and constants before it.
pub(crate) fn constants(include: &Path) -> Vec<(String, i64)> {
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

#[cfg(test)]
mod tests {
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

    #[test]
    fn enumerators_from_a_uapi_enum() {
        // (The W line names the enum's type: declared maps names to files.)
        let t = ksee_core::types::Types::parse(
            "T\t1\tenum\tklock\t4\t\nV\t1\tIORING_OP_READ\t1\nV\t1\tOTHER\t3\nW\tklock\tinclude/uapi/linux/io_uring.h\t10\n");
        let got: HashMap<String, i64> = enumerators(&t).into_iter().collect();
        assert_eq!(got.get("IORING_OP_READ"), Some(&1));
        assert!(!got.contains_key("OTHER"));
        // (An enum another header declares is not a constant.)
        let t = ksee_core::types::Types::parse(
            "T\t1\tenum\tklock\t4\t\nV\t1\tIORING_OP_READ\t1\nW\tklock\tkernel/locking.c\t10\n");
        assert!(enumerators(&t).is_empty());
    }
}
