//! `ksee build` and `ksee test`: the build, in order. Every step asks the
//! tool that knows its own dependencies (kbuild, cargo), or checks whether
//! its output is older than its inputs.
//!
//!   linux/                 Linux, the stable tree's LINUX_TAG, fetched once, with linux.patch applied
//!   linux.patch            the port: linux/ as it differs from the release (written by `ksee patch`)
//!   plugin/ksee-trace.so   the LLVM pass that instruments the kernel
//!   linux/vmlinux          the kernel, instrumented (kbuild)
//!
//! and the site, in target/site (what `ksee serve` serves and `ksee deploy`
//! pushes): links to web/'s files (the page's own), and
//!   pkg/                   the runtime, and wasm-bindgen's glue (cargo)
//!   vmlinux.gz, src/       the kernel for the page (pack; target/vmlinux)
//!   initramfs.cpio.gz      /init (user/init.c), /bin/prog (user/prog.c: the page's program), and an ext4 disk image
//!                          (/disk.img, mkfs.ext4), from target/initramfs/
//!
//! (Gzipped by the build: hosts compress them less, or not at all; the
//! page unpacks them.)
//!   examples.json          the examples (user/examples/*.json)
//!
//! The tools setup.sh installs, first on every command's PATH.
use std::path::Path;
use std::process::Command;
use std::sync::LazyLock;

fn brew_prefix(formula: &str) -> String {
    String::from_utf8_lossy(&stdout(Command::new("brew").args(["--prefix", formula])))
        .trim().to_string()
}

/// LLVM 22's binaries. An override is useful for a custom toolchain, but is
/// absolute so builds do not silently change with their working directory.
static LLVM_BIN: LazyLock<String> = LazyLock::new(|| {
    if let Ok(bin) = std::env::var("KSEE_LLVM_BIN") {
        if !Path::new(&bin).is_absolute() || !Path::new(&bin).is_dir() {
            crate::fail("KSEE_LLVM_BIN must be an absolute directory");
        }
        bin
    } else if cfg!(target_os = "macos") {
        format!("{}/bin", brew_prefix("llvm@22"))
    } else {
        "/usr/lib/llvm-22/bin".into()
    }
});

/// setup.sh's dirs first, then the caller's PATH. macOS also needs Homebrew's
/// lld, GNU make and sed, and mkfs.ext4.
static PATH: LazyLock<String> = LazyLock::new(|| {
    let mut dirs = vec![(*LLVM_BIN).clone()];
    if cfg!(target_os = "macos") {
        dirs.extend([
            format!("{}/bin", brew_prefix("lld@22")),
            format!("{}/libexec/gnubin", brew_prefix("make")),
            format!("{}/libexec/gnubin", brew_prefix("gnu-sed")),
            format!("{}/sbin", brew_prefix("e2fsprogs")),
        ]);
    }
    format!("{}:{}", dirs.join(":"), std::env::var("PATH").unwrap_or_default())
});

/// A command with the tools on its PATH.
fn cmd(program: &str) -> Command {
    let mut c = Command::new(program);
    c.env("PATH", &*PATH);
    c
}

/// `cmd` as a line: its program and arguments, spaced.
fn line(cmd: &Command) -> String {
    std::iter::once(cmd.get_program()).chain(cmd.get_args())
        .map(|s| s.to_string_lossy()).collect::<Vec<_>>().join(" ")
}

/// Runs `cmd`, said first, its output shown.
pub fn run(cmd: &mut Command) {
    eprintln!("+ {}", line(cmd));
    let status = cmd.status().unwrap_or_else(|e| crate::fail(&format!("{}: {e}", line(cmd))));
    if !status.success() {
        crate::fail(&format!("{} failed: {status}", line(cmd)));
    }
}

/// Runs `cmd`, said first, and returns its stdout; stderr remains visible.
fn stdout(cmd: &mut Command) -> Vec<u8> {
    eprintln!("+ {}", line(cmd));
    let out = cmd.stderr(std::process::Stdio::inherit()).output()
        .unwrap_or_else(|e| crate::fail(&format!("{}: {e}", line(cmd))));
    if !out.status.success() {
        crate::fail(&format!("{} failed: {}", line(cmd), out.status));
    }
    out.stdout
}

/// The site: what the page loads.
pub const SITE: &str = "target/site";

/// Whether `out` is missing or older than any of `inputs`.
fn stale(out: &str, inputs: &[&str]) -> bool {
    let time = |p: &str| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let Some(t) = time(out) else { return true };
    inputs.iter().any(|i| time(i).is_none_or(|ti| ti > t))
}

fn any_stale(outputs: &[&str], inputs: &[&str]) -> bool {
    outputs.iter().any(|out| stale(out, inputs))
}

fn check_llvm() {
    let version = String::from_utf8_lossy(&stdout(cmd("llvm-config").arg("--version"))).trim().to_string();
    if version.split('.').next() != Some("22") {
        crate::fail(&format!("LLVM 22 is required, found {version}"));
    }
}

const CACHED_LINUX: &str = "KSEE_CACHED_LINUX";

fn cached_kernel(root: &Path, hit: Option<&str>) -> bool {
    hit == Some("true") &&
        ["Makefile", ".config", "vmlinux"].iter().all(|p| root.join(p).is_file()) &&
        root.join("usr/include").is_dir()
}

fn trace_plugin() -> (String, std::path::PathBuf) {
    if stale("plugin/ksee-trace.so", &["plugin/ksee-trace.cpp"]) {
        let flags = stdout(cmd("llvm-config").arg("--cxxflags"));
        let mut plugin = cmd("clang++");
        plugin.args(["-shared", "-fPIC", "-O2", "plugin/ksee-trace.cpp", "-o", "plugin/ksee-trace.so"]);
        // (macOS links -shared eagerly: leave the LLVM symbols to clang,
        // which loads the pass.)
        if cfg!(target_os = "macos") {
            plugin.arg("-Wl,-undefined,dynamic_lookup");
        }
        run(plugin.args(String::from_utf8_lossy(&flags).split_whitespace()));
    }
    // A hash in the flags makes kbuild rebuild when the plugin changes.
    let id = format!("{:016x}", fnv64(&std::fs::read("plugin/ksee-trace.so").unwrap_or_default()));
    let path = Path::new("plugin/ksee-trace.so").canonicalize().unwrap();
    (id, path)
}

fn kernel() {
    linux_tree();

    let cache_hit = std::env::var(CACHED_LINUX).ok();
    let cached = cached_kernel(Path::new("linux"), cache_hit.as_deref());
    if cache_hit.as_deref() == Some("true") && !cached {
        eprintln!("+ (cache) incomplete Linux cache; rebuilding");
    }
    if cached {
        eprintln!("+ (cache) Linux kernel and headers");
    } else {
        let (id, plugin) = trace_plugin();
        // (Inlining only what the source asks for, `inline`: the compiler's
        // own choices would hide calls, with their arguments, as inlined code.)
        let jobs = format!("-j{}", std::thread::available_parallelism().map_or(4, |n| n.get()));
        let kmake = |args: &[&str]| {
            let mut c = cmd("make");
            c.args(["-C", "linux", "ARCH=wasm", "LLVM=1",
                    &format!("KCFLAGS=-g -finline-hint-functions -fpass-plugin={} -DKSEE_ID={id}", plugin.display())])
                .arg(&jobs)
                .args(args);
            c
        };
        // (Every Kconfig the port has: a new one re-runs defconfig too.)
        let mut config = vec!["linux/arch/wasm/configs/defconfig".to_string()];
        for e in std::fs::read_dir("linux/arch/wasm").into_iter().flatten().flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with("Kconfig") {
                config.push(format!("linux/arch/wasm/{n}"));
            }
        }
        if stale("linux/.config", &config.iter().map(String::as_str).collect::<Vec<_>>()) {
            run(&mut kmake(&["defconfig"]));
        }
        run(&mut kmake(&[]));
        // (Its UAPI headers: the pack's constants, the programs' syscalls.)
        run(&mut kmake(&["headers"]));
    }
}

pub fn build() {
    check_llvm();
    kernel();
    site();
    // (The runtime and the page, one module: the page calls the Machine.)
    run(cmd("cargo").args(["build", "--locked", "--release", "-p", "ksee-page",
        "--target", "wasm32-unknown-unknown"]));
    bindgen("target/wasm32-unknown-unknown/release/ksee_page.wasm", &format!("{SITE}/pkg"));

    // (Packed again for a new kernel or pack implementation, or to restore
    // either required result after a partial clean.)
    let me = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
    let pack_inputs = ["linux/vmlinux", me.as_str()];
    let sources = format!("{SITE}/src");
    if any_stale(&["target/vmlinux", &sources], &pack_inputs) {
        eprintln!("+ (pack) linux -> target/vmlinux, {sources}");
        crate::pack::kernel(Path::new("linux"), Path::new("target/vmlinux"), Path::new(&sources));
    }
    let packed = format!("{SITE}/vmlinux.gz");
    if stale(&packed, &["target/vmlinux"]) {
        gzip("target/vmlinux", &packed);
    }
    // The programs (/init, /bin/prog): plain C, no C library (their syscalls:
    // user/syscall.h), with the kernel's UAPI headers.
    let programs = ["init", "prog"];
    // (And this command: the archive's entries are in it, cpio().)
    let inputs = ["user/init.c", "user/prog.c", "user/syscall.h", "target/release/ksee"];
    let dir = "target/initramfs";
    let cpio_out = format!("{dir}/initramfs.cpio");
    if stale(&cpio_out, &inputs) {
        std::fs::create_dir_all(dir).unwrap_or_else(|e| crate::fail(&format!("{dir}: {e}")));
        for p in programs {
            run(cmd("clang")
                // (Position-independent, in the kernel's memory: see binfmt_wasm.c.)
                .args(["--target=wasm32", "-O2", "-g0", "-ffreestanding", "-nostdlib", "-fPIC", "-Wl,-pie",
                       "-Wl,--experimental-pic", "-Wl,--import-memory", "-Wl,--no-entry",
                       "-nostdinc", "-isystem", "linux/usr/include",
                       "-o", &format!("{dir}/{p}.wasm"), &format!("user/{p}.c")]));
        }
        // A file in the (in-memory) root, for the examples: /note.txt (its
        // bytes go to the archive; the disk's own note.txt is below).
        // The ext4 disk /init mounts (a file, through the loop device), with
        // files for the examples to start from: note.txt, dir/a.txt.
        let disk = format!("{dir}/disk.img");
        let _ = std::fs::remove_file(&disk);
        let files = format!("{dir}/disk");
        let _ = std::fs::remove_dir_all(&files);
        for (path, text) in [("note.txt", "hello from the disk\n"), ("dir/a.txt", "a\n")] {
            let p = Path::new(&files).join(path);
            std::fs::create_dir_all(p.parent().unwrap()).and_then(|_| std::fs::write(&p, text))
                .unwrap_or_else(|e| crate::fail(&format!("{}: {e}", p.display())));
        }
        // (Initialized in full: no background work after the mount, ext4lazyinit's.)
        run(cmd("mkfs.ext4").args(["-F", "-b", "1024", "-L", "ksee", "-N", "32", "-d", &files,
                                            "-E", "lazy_itable_init=0,lazy_journal_init=0", &disk, "2M"]));
        // The archive: the programs, a note, and the disk.
        eprintln!("+ (cpio) -> {cpio_out}");
        std::fs::write(&cpio_out, cpio(Path::new(dir)))
            .unwrap_or_else(|e| crate::fail(&format!("{cpio_out}: {e}")));
    }
    let compressed = format!("{SITE}/initramfs.cpio.gz");
    if stale(&compressed, &[&cpio_out]) {
        gzip(&cpio_out, &compressed);
    }
    examples();
}

/// The release the port is a patch to: the stable tree's tag, and the
/// commit it must be (what is fetched, checked).
const LINUX_TREE: &str = "https://git.kernel.org/pub/scm/linux/kernel/git/stable/linux.git";
const LINUX_TAG: &str = "v7.2.7";
const LINUX_COMMIT: &str = "f42acb3678424d1e08f6ed27c0d8ba8a125e14d6";

/// linux/: the release, fetched (a shallow git repo: its one commit) and
/// linux.patch applied, the first time (only then: to re-apply an updated
/// patch, remove linux/ first).
fn linux_tree() {
    if !Path::new("linux/Makefile").exists() {
        eprintln!("+ (fetch) {LINUX_TREE} {LINUX_TAG} -> linux/");
        std::fs::create_dir_all("linux").unwrap_or_else(|e| crate::fail(&format!("linux/: {e}")));
        git(&["init"]);
        git(&["fetch", "--depth=1", LINUX_TREE, "tag", LINUX_TAG]);
        let commit = git(&["rev-parse", &format!("{LINUX_TAG}^{{commit}}")]);
        if String::from_utf8_lossy(&commit).trim() != LINUX_COMMIT {
            crate::fail(&format!("{LINUX_TAG}: not {LINUX_COMMIT}"));
        }
        git(&["checkout", LINUX_TAG]);
        git(&["apply", "../linux.patch"]);
    }
}

/// linux.patch: the port, all of it (the files new too) as linux/ differs
/// from the release. Written by `ksee patch`, never by `ksee build`.
pub fn patch() {
    if !Path::new("linux/Makefile").exists() {
        crate::fail("no linux/: run ./ksee.sh build first");
    }
    git(&["add", "-A", "--intent-to-add"]);
    let diff = git(&["diff", "--no-color", LINUX_TAG]);
    if std::fs::read("linux.patch").ok().as_deref() == Some(&diff[..]) {
        return;
    }
    eprintln!("+ (patch) linux -> linux.patch");
    std::fs::write("linux.patch", &diff).unwrap_or_else(|e| crate::fail(&format!("linux.patch: {e}")));
}

fn git(args: &[&str]) -> Vec<u8> {
    stdout(cmd("git").arg("-C").arg("linux").args(args))
}

/// wasm-bindgen's glue for `wasm`, in `out` (the crate, at ksee-page's
/// wasm-bindgen version: the two must match).
fn bindgen(wasm: &str, out: &str) {
    run(cmd("wasm-bindgen").args(["--target", "web", "--no-typescript", "--out-dir", out, wasm]));
}

fn gzip(from: &str, to: &str) {
    let out = std::fs::File::create(to).unwrap_or_else(|e| crate::fail(&format!("{to}: {e}")));
    run(cmd("gzip").args(["-9nc", from]).stdout(out));
}

/// The initramfs, as a newc ("070701") archive: the console (the kernel
/// opens it before init; the kernel's own initramfs is empty), the
/// programs, a note, and the disk image, with their modes; all times are 0.
fn cpio(dir: &Path) -> Vec<u8> {
    let file = |name: &str| std::fs::read(dir.join(name))
        .unwrap_or_else(|e| crate::fail(&format!("{}: {e}", dir.join(name).display())));
    // (Archive path, mode, device (major, minor), data; a directory's data is empty.)
    let entries = [
        ("/dev", 0o040755, (0, 0), Vec::new()),
        ("/dev/console", 0o020600, (5, 1), Vec::new()),
        ("/init", 0o100755, (0, 0), file("init.wasm")),
        ("/note.txt", 0o100644, (0, 0), b"hello from memory\n".to_vec()),
        ("/bin", 0o040755, (0, 0), Vec::new()),
        ("/bin/prog", 0o100755, (0, 0), file("prog.wasm")),
        ("/mnt", 0o040755, (0, 0), Vec::new()),
        ("/disk.img", 0o100644, (0, 0), file("disk.img")),
        ("TRAILER!!!", 0, (0, 0), Vec::new()),
    ];
    let mut out = Vec::new();
    for (i, (name, mode, (major, minor), data)) in entries.iter().enumerate() {
        let nlink = if mode & 0o170000 == 0o040000 { 2 } else { 1 };
        out.extend_from_slice(format!("070701{:08X}{mode:08X}0000000000000000{nlink:08X}00000000{:08X}0000000300000001{major:08X}{minor:08X}{:08X}00000000",
            i + 1, data.len(), name.len() + 1).as_bytes());
        out.extend_from_slice(name.as_bytes());
        // (The name's NUL, then pad the offset: the header is 110 bytes.
        // The file ends padded to a block, as gen_init_cpio left it.)
        out.push(0);
        while out.len() % 4 != 0 {
            out.push(0);
        }
        out.extend_from_slice(data);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    while out.len() % 512 != 0 {
        out.push(0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The headers back: each entry's name, mode and data.
    fn entries(archive: &[u8]) -> Vec<(String, u32, Vec<u8>)> {
        let mut entries = Vec::new();
        let mut at = 0;
        let align = |at: usize| {
            let mut at = at;
            while at % 4 != 0 {
                assert_eq!(archive[at], 0);
                at += 1;
            }
            at
        };
        loop {
            let head = std::str::from_utf8(&archive[at..at + 110]).unwrap();
            assert_eq!(&head[..6], "070701");
            let num = |r: std::ops::Range<usize>| u32::from_str_radix(&head[r], 16).unwrap();
            let (size, namesize) = (num(54..62) as usize, num(94..102) as usize);
            at += 110;
            let name = std::str::from_utf8(&archive[at..at + namesize - 1]).unwrap().to_string();
            at = align(at + namesize);
            let data = archive[at..at + size].to_vec();
            at = align(at + size);
            let last = name == "TRAILER!!!";
            entries.push((name, num(14..22), data));
            if last {
                return entries;
            }
        }
    }

    #[test]
    fn cpio_round_trip() {
        let dir = std::env::temp_dir().join(format!("ksee-cpio-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("init.wasm"), b"\0asm").unwrap();
        std::fs::write(dir.join("prog.wasm"), b"\0asm").unwrap();
        std::fs::write(dir.join("disk.img"), b"disk").unwrap();
        let entries = entries(&cpio(&dir));
        assert_eq!(entries.iter().map(|e| (e.0.as_str(), e.1)).collect::<Vec<_>>(),
            [("/dev", 0o040755), ("/dev/console", 0o020600), ("/init", 0o100755), ("/note.txt", 0o100644), ("/bin", 0o040755),
             ("/bin/prog", 0o100755), ("/mnt", 0o040755), ("/disk.img", 0o100644), ("TRAILER!!!", 0)]);
        assert_eq!(entries[2].2.as_slice(), b"\0asm");
        assert_eq!(entries[3].2.as_slice(), b"hello from memory\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stale_outputs_are_detected() {
        let dir = std::env::temp_dir().join(format!("ksee-stale-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("input");
        let first = dir.join("first");
        let second = dir.join("second");
        let input_s = input.to_string_lossy();
        let first_s = first.to_string_lossy();
        let second_s = second.to_string_lossy();

        std::fs::write(&first, b"output").unwrap();
        assert!(stale(&second_s, &[&input_s]));
        assert!(stale(&first_s, &[&input_s]));

        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&input, b"input").unwrap();
        assert!(stale(&first_s, &[&input_s]));

        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&first, b"new output").unwrap();
        assert!(!stale(&first_s, &[&input_s]));
        assert!(any_stale(&[&first_s, &second_s], &[&input_s]));

        std::fs::write(&second, b"new output").unwrap();
        assert!(!any_stale(&[&first_s, &second_s], &[&input_s]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_a_complete_exact_linux_cache_is_reused() {
        let root = std::env::temp_dir().join(format!("ksee-linux-cache-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("usr/include")).unwrap();
        for file in ["Makefile", ".config", "vmlinux"] {
            std::fs::write(root.join(file), "").unwrap();
        }

        for value in [None, Some(""), Some("false"), Some("TRUE"), Some("1")] {
            assert!(!cached_kernel(&root, value));
        }
        assert!(cached_kernel(&root, Some("true")));

        std::fs::remove_file(root.join("vmlinux")).unwrap();
        assert!(!cached_kernel(&root, Some("true")));
        std::fs::write(root.join("vmlinux"), "").unwrap();
        std::fs::remove_dir(root.join("usr/include")).unwrap();
        std::fs::write(root.join("usr/include"), "").unwrap();
        assert!(!cached_kernel(&root, Some("true")));
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// The course: its chapters, each its examples (user/examples/NAME.json)
/// in order, from the simplest.
const EXAMPLES: &[(&str, &[&str])] = &[
    ("Start here", &["umask", "umask-inside"]),
    ("A process", &["getpid", "prctl", "exec", "thread"]),
    ("Files, in memory", &["hello", "open-close", "dup", "read", "write", "open-fails", "create", "dir"]),
    ("Files on a disk (ext4)", &["ext4-read", "ext4-create", "ext4-write", "durable", "ext4-dir", "link", "remove"]),
    ("Waiting and waking", &["sleep", "futex", "join", "pipe", "kill"]),
    ("Your own", &["yours"]),
];

/// examples.json: the examples (user/examples/*.json: each a list
/// of syscalls for /init to make, one at a time), in the course's order,
/// with their names and chapters (`group`); any other after them.
fn examples() {
    eprintln!("+ (examples) user/examples/*.json -> {SITE}/examples.json");
    let course: Vec<(&str, &str)> = EXAMPLES.iter().flat_map(|&(group, names)| names.iter().map(move |&n| (n, group))).collect();
    let mut index: Vec<(usize, String)> = std::fs::read_dir("user/examples").into_iter().flatten().flatten()
        .map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).map(|p| {
            let name = p.file_stem().unwrap().to_string_lossy().into_owned();
            let text = String::from_utf8_lossy(&crate::read(&p)).into_owned();
            let k = course.iter().position(|&(n, _)| n == name).unwrap_or(course.len());
            let group = course.get(k).map_or("", |&(_, g)| g);
            let entry = format!("  {{ \"name\": \"{name}\", \"group\": \"{group}\", {}", text.trim().strip_prefix('{').unwrap_or("}").trim_start());
            (k, entry)
        }).collect();
    index.sort();
    let index: Vec<String> = index.into_iter().map(|(_, e)| e).collect();
    let out = format!("{SITE}/examples.json");
    std::fs::write(&out, format!("[\n{}\n]\n", index.join(",\n")))
        .unwrap_or_else(|e| crate::fail(&format!("{out}: {e}")));
}

/// Links in the site to web/'s files (so an edit there needs no build), and
/// none to files no longer there.
fn site() {
    eprintln!("+ (site) web/* -> {SITE}/");
    std::fs::create_dir_all(SITE).unwrap_or_else(|e| crate::fail(&format!("{SITE}: {e}")));
    for e in std::fs::read_dir(SITE).into_iter().flatten().flatten() {
        if e.path().is_symlink() && !e.path().exists() {
            let _ = std::fs::remove_file(e.path());
        }
    }
    for e in std::fs::read_dir("web").into_iter().flatten().flatten() {
        let link = Path::new(SITE).join(e.file_name());
        if !link.is_symlink() {
            std::os::unix::fs::symlink(Path::new("../../web").join(e.file_name()), &link)
                .unwrap_or_else(|err| crate::fail(&format!("{}: {err}", link.display())));
        }
    }
}

/// Core's and this command's unit tests, then build, then `names` run in
/// the page (crates/ksee-page built with them, as target/test/site) in
/// headless Chromium ($CHROME, see chrome.rs).
pub fn test(names: &[String]) {
    run(cmd("cargo").args(["test", "--locked", "--release", "-p", "ksee-core", "-p", "ksee-cli"]));
    build();
    test_site();
    for name in names {
        if !crate::chrome::test(name) {
            crate::fail(&format!("the {name} test failed"));
        }
    }
}

/// target/test/site: the site, its module built with the tests.
fn test_site() {
    // (Its own target directory: the site's module, built without them, stays as it is.)
    run(cmd("cargo").args(["build", "--locked", "--release", "-p", "ksee-page", "--features", "selftest",
        "--target", "wasm32-unknown-unknown", "--target-dir", "target/test/build"]));
    bindgen("target/test/build/wasm32-unknown-unknown/release/ksee_page.wasm", "target/test/pkg");
    // (The site with the tests: the site's files, its module the one built with them.)
    let site = Path::new("target/test/site");
    let _ = std::fs::remove_dir_all(site);
    std::fs::create_dir_all(site).unwrap_or_else(|e| crate::fail(&format!("{}: {e}", site.display())));
    for e in std::fs::read_dir(SITE).into_iter().flatten().flatten() {
        let name = e.file_name();
        let from = if name == "pkg" { Path::new("../pkg").to_path_buf() } else { Path::new("../../site").join(&name) };
        std::os::unix::fs::symlink(from, site.join(&name)).unwrap_or_else(|e| crate::fail(&format!("{}: {e}", site.display())));
    }
}

fn fnv64(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf29ce484222325, |h, &b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}
