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

/// setup.sh's dirs first, then the caller's PATH. macOS: brew's LLVM, lld,
/// GNU make and sed, mkfs.ext4; Linux: the LLVM setup.sh installs.
static PATH: LazyLock<String> = LazyLock::new(|| {
    let dirs: Vec<String> = if cfg!(target_os = "macos") {
        let brew = String::from_utf8_lossy(&output(Command::new("brew").arg("--prefix"))).trim().to_string();
        ["opt/llvm/bin", "opt/lld/bin", "opt/make/libexec/gnubin", "opt/gnu-sed/libexec/gnubin", "opt/e2fsprogs/sbin"]
            .iter().map(|d| format!("{brew}/{d}")).collect()
    } else {
        vec!["/usr/lib/llvm-23/bin".into()]
    };
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

/// Runs `cmd`, said first, for its stdout (its stderr shown, its stdout said
/// after when short, else its size).
fn output(cmd: &mut Command) -> Vec<u8> {
    eprintln!("+ {}", line(cmd));
    let out = cmd.stderr(std::process::Stdio::inherit()).output()
        .unwrap_or_else(|e| crate::fail(&format!("{}: {e}", line(cmd))));
    if !out.status.success() {
        crate::fail(&format!("{} failed: {}", line(cmd), out.status));
    }
    match String::from_utf8(out.stdout) {
        Ok(s) if s.lines().count() <= 20 => {
            if !s.is_empty() {
                eprintln!("{s}");
            }
            s.into_bytes()
        }
        Ok(s) => {
            eprintln!("({} bytes)", s.len());
            s.into_bytes()
        }
        Err(e) => {
            eprintln!("({} bytes)", e.as_bytes().len());
            e.into_bytes()
        }
    }
}

/// The site: what the page loads.
pub const SITE: &str = "target/site";

/// Whether `out` is missing or older than any of `inputs`.
fn stale(out: &str, inputs: &[&str]) -> bool {
    let time = |p: &str| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let Some(t) = time(out) else { return true };
    inputs.iter().any(|i| time(i).is_none_or(|ti| ti > t))
}

pub fn build() {
    linux();

    if stale("plugin/ksee-trace.so", &["plugin/ksee-trace.cpp"]) {
        let flags = output(cmd("llvm-config").arg("--cxxflags"));
        let mut plugin = cmd("clang++");
        plugin.args(["-shared", "-fPIC", "-O2", "plugin/ksee-trace.cpp", "-o", "plugin/ksee-trace.so"]);
        // (macOS links -shared eagerly: leave the LLVM symbols to clang,
        // which loads the pass.)
        if cfg!(target_os = "macos") {
            plugin.arg("-Wl,-undefined,dynamic_lookup");
        }
        run(plugin.args(String::from_utf8_lossy(&flags).split_whitespace()));
    }

    // A hash of the plugin in the flags makes kbuild rebuild the kernel
    // when it changes.
    let id = format!("{:016x}", fnv64(&std::fs::read("plugin/ksee-trace.so").unwrap_or_default()));
    let plugin = Path::new("plugin/ksee-trace.so").canonicalize().unwrap();
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
    if stale("linux/.config", &["linux/arch/wasm/configs/defconfig", "linux/arch/wasm/Kconfig", "linux/arch/wasm/Kconfig.debug"]) {
        run(&mut kmake(&["defconfig"]));
    }
    run(&mut kmake(&[]));
    // (Its UAPI headers: the pack's constants, the programs' syscalls.)
    run(&mut kmake(&["headers"]));

    site();
    // (The runtime and the page, one module: the page calls the Machine.)
    run(cmd("cargo").args(["build", "--release", "-p", "ksee-page",
        "--target", "wasm32-unknown-unknown"]));
    bindgen("target/wasm32-unknown-unknown/release/ksee_page.wasm", &format!("{SITE}/pkg"));

    // (Packed again for a new kernel, or a new pack: this program.)
    let me = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
    if stale("target/vmlinux", &["linux/vmlinux", &me]) {
        eprintln!("+ (pack) linux -> {SITE}");
        crate::pack::pack(Path::new("linux"), Path::new(SITE));
        std::fs::rename(format!("{SITE}/vmlinux"), "target/vmlinux")
            .unwrap_or_else(|e| crate::fail(&format!("target/vmlinux: {e}")));
        gzip("target/vmlinux", &format!("{SITE}/vmlinux.gz"));
    }
    // The programs (/init, /bin/prog): plain C, no C library (their syscalls:
    // user/syscall.h), with the kernel's UAPI headers.
    let programs = ["init", "prog"];
    let inputs = ["user/init.c", "user/prog.c", "user/syscall.h", "user/initramfs.list"];
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
        // A file in the (in-memory) root, for the examples: /note.txt.
        std::fs::write(format!("{dir}/note.txt"), "hello from memory\n").unwrap_or_else(|e| crate::fail(&format!("{dir}: {e}")));
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
        // (The list's paths are relative to the dir.)
        eprintln!("+ (cpio) user/initramfs.list -> {cpio_out}");
        let list = crate::read(Path::new("user/initramfs.list"));
        std::fs::write(&cpio_out, cpio(&String::from_utf8_lossy(&list), Path::new(dir)))
            .unwrap_or_else(|e| crate::fail(&format!("{cpio_out}: {e}")));
        gzip(&cpio_out, &format!("{SITE}/initramfs.cpio.gz"));
    }
    examples();
}

/// The release the port is a patch to: the stable tree's tag, and the
/// commit it must be (what is fetched, checked).
const LINUX_TREE: &str = "https://git.kernel.org/pub/scm/linux/kernel/git/stable/linux.git";
const LINUX_TAG: &str = "v7.2.7";
const LINUX_COMMIT: &str = "f42acb3678424d1e08f6ed27c0d8ba8a125e14d6";

/// linux/: the release, fetched (a shallow git repo: its one commit) and
/// linux.patch applied, the first time.
fn linux() {
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
    git(&["add", "-A", "--intent-to-add"]);
    let diff = git(&["diff", "--no-color", LINUX_TAG]);
    eprintln!("+ (patch) linux -> linux.patch");
    std::fs::write("linux.patch", &diff).unwrap_or_else(|e| crate::fail(&format!("linux.patch: {e}")));
}

fn git(args: &[&str]) -> Vec<u8> {
    output(cmd("git").arg("-C").arg("linux").args(args))
}

/// wasm-bindgen's glue for `wasm`, in `out` (the crate, at ksee-page's
/// wasm-bindgen version: the two must match).
fn bindgen(wasm: &str, out: &str) {
    eprintln!("+ (bindgen) {wasm} -> {out}");
    wasm_bindgen_cli_support::Bindgen::new().input_path(wasm).web(true).and_then(|b| b.typescript(false).generate(out))
        .unwrap_or_else(|e| crate::fail(&format!("wasm-bindgen {wasm}: {e}")));
}

fn gzip(from: &str, to: &str) {
    let out = std::fs::File::create(to).unwrap_or_else(|e| crate::fail(&format!("{to}: {e}")));
    run(cmd("gzip").args(["-9nc", from]).stdout(out));
}

/// The archive for the list's entries (user/initramfs.list: `file NAME
/// LOCATION MODE UID GID`, `dir NAME MODE UID GID`, `nod NAME MODE UID GID
/// TYPE MAJ MIN` with TYPE b or c, `slink NAME TARGET MODE UID GID`; modes
/// octal), as a newc ("070701") archive: what gen_init_cpio wrote. Names lose
/// their leading `/`; the locations live under `dir`; all times are 0.
fn cpio(list: &str, dir: &Path) -> Vec<u8> {
    // (The kernel's own file types, as octal: no libc here.)
    const REG: u32 = 0o100000;
    const DIR: u32 = 0o040000;
    const LNK: u32 = 0o120000;
    const BLK: u32 = 0o060000;
    const CHR: u32 = 0o020000;
    let mut out = Vec::new();
    let mut ino = 0u32;
    let mut entry = |name: &str, mode: u32, uid: u32, gid: u32, nlink: u32, maj: u32, min: u32, data: &[u8]| {
        ino += 1;
        let name = name.strip_prefix('/').unwrap_or(name);
        out.extend_from_slice(format!("070701{ino:08X}{mode:08X}{uid:08X}{gid:08X}{nlink:08X}00000000{:08X}0000000300000001{maj:08X}{min:08X}{:08X}00000000",
            data.len(), name.len() + 1).as_bytes());
        out.extend_from_slice(name.as_bytes());
        // (The name's NUL, then pad the offset: the header is 110 bytes.)
        out.push(0);
        while out.len() % 4 != 0 {
            out.push(0);
        }
        out.extend_from_slice(data);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    };
    let num = |w: &str| w.parse::<u32>().unwrap_or_else(|_| crate::fail(&format!("initramfs.list: {w}: not a number")));
    let oct = |w: &str| u32::from_str_radix(w, 8).unwrap_or_else(|_| crate::fail(&format!("initramfs.list: {w}: not octal")));
    for line in list.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let w: Vec<&str> = line.split_whitespace().collect();
        match w[..] {
            ["file", name, loc, mode, uid, gid] => {
                let data = std::fs::read(dir.join(loc))
                    .unwrap_or_else(|e| crate::fail(&format!("{}: {e}", dir.join(loc).display())));
                entry(name, REG | oct(mode), num(uid), num(gid), 1, 0, 0, &data);
            }
            ["dir", name, mode, uid, gid] => entry(name, DIR | oct(mode), num(uid), num(gid), 2, 0, 0, &[]),
            ["nod", name, mode, uid, gid, t, maj, min] => entry(name,
                match t { "b" => BLK, "c" => CHR, _ => crate::fail(&format!("initramfs.list: {t}: neither b nor c")) } | oct(mode),
                num(uid), num(gid), 1, num(maj), num(min), &[]),
            ["slink", name, target, mode, uid, gid] => {
                let mut data = target.as_bytes().to_vec();
                data.push(0);
                entry(name, LNK | oct(mode), num(uid), num(gid), 1, 0, 0, &data);
            }
            _ => crate::fail(&format!("initramfs.list: {line}: neither file, dir, nod nor slink")),
        }
    }
    entry("TRAILER!!!", 0, 0, 0, 1, 0, 0, &[]);
    // (As gen_init_cpio: pad the file to a block.)
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
        let mut align = |at: &mut usize| {
            while *at % 4 != 0 {
                assert_eq!(archive[*at], 0);
                *at += 1;
            }
        };
        loop {
            let head = std::str::from_utf8(&archive[at..at + 110]).unwrap();
            assert_eq!(&head[..6], "070701");
            let num = |r: std::ops::Range<usize>| u32::from_str_radix(&head[r], 16).unwrap();
            let (size, namesize) = (num(54..62) as usize, num(94..102) as usize);
            at += 110;
            let name = std::str::from_utf8(&archive[at..at + namesize - 1]).unwrap().to_string();
            at += namesize;
            align(&mut at);
            let data = archive[at..at + size].to_vec();
            at += size;
            align(&mut at);
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
        std::fs::write(dir.join("hello"), b"hi\n").unwrap();
        let entries = entries(&cpio("dir /d 0755 0 0\nfile /d/hello hello 0644 0 0\nslink /d/link hello 0777 0 0\n", &dir));
        assert_eq!(entries.len(), 4);
        assert_eq!((entries[0].0.as_str(), entries[0].1, entries[0].2.len()), ("d", 0o040755u32, 0));
        assert_eq!((entries[1].0.as_str(), entries[1].1, entries[1].2.as_slice()), ("d/hello", 0o100644u32, b"hi\n".as_slice()));
        assert_eq!((entries[2].0.as_str(), entries[2].1, entries[2].2.as_slice()), ("d/link", 0o120777u32, b"hello\0".as_slice()));
        assert_eq!((entries[3].0.as_str(), entries[3].2.len()), ("TRAILER!!!", 0));
        let _ = std::fs::remove_dir_all(&dir);
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
    ("Requests in memory (io_uring)", &["uring-setup", "uring-read", "uring-worker"]),
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

/// Build, then: core's and this command's unit tests, then the tests run in the page
/// (crates/ksee-page built with them, as target/test/site) in headless
/// Chromium ($CHROME, see chrome.rs): the kernel's (its recordings, every
/// example); with `ui`, also the page's (the read example, a changed
/// program, the pipe example, a phone's screen), a screenshot each in
/// target/.
pub fn test(ui: bool) {
    build();
    run(cmd("cargo").args(["test", "-p", "ksee-core", "-p", "ksee-cli"]));
    // (Its own target directory: the site's module, built without them, stays as it is.)
    run(cmd("cargo").args(["build", "--release", "-p", "ksee-page", "--features", "selftest",
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
    for &(name, ..) in crate::chrome::TESTS.iter().filter(|t| ui || t.0 == "kernel") {
        let out = (name != "kernel").then(|| format!("target/ui-{name}.png").into());
        if !crate::chrome::Shot::test(name, out).run() {
            crate::fail(&format!("the {name} test failed"));
        }
    }
}

fn fnv64(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf29ce484222325, |h, &b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}
