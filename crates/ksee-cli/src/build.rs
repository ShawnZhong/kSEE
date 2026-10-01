//! `ksee build` and `ksee test`: the build, in order. Every step asks the
//! tool that knows its own dependencies (kbuild, cargo), or checks whether
//! its output is older than its inputs.
//!
//!   linux/                 Linux, the stable tree's LINUX_TAG, fetched once, with linux.patch applied
//!   linux.patch            the port: linux/ as it differs from the release (written again as it changes)
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
//! $LLVM is the LLVM version suffix (default -23).
use std::path::Path;
use std::process::Command;

fn env(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.into())
}

pub fn run(cmd: &mut Command) {
    let status = cmd.status().unwrap_or_else(|e| crate::fail(&format!("{:?}: {e}", cmd.get_program())));
    if !status.success() {
        crate::fail(&format!("{:?} failed", cmd.get_program()));
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
    let llvm = env("LLVM", "-23");
    linux();

    if stale("plugin/ksee-trace.so", &["plugin/ksee-trace.cpp"]) {
        eprintln!("  PLUGIN  plugin/ksee-trace.so");
        let flags = Command::new(format!("llvm-config{llvm}")).arg("--cxxflags").output()
            .unwrap_or_else(|e| crate::fail(&format!("llvm-config{llvm}: {e}"))).stdout;
        run(Command::new(format!("clang++{llvm}"))
            .args(["-shared", "-fPIC", "-O2", "plugin/ksee-trace.cpp", "-o", "plugin/ksee-trace.so"])
            .args(String::from_utf8_lossy(&flags).split_whitespace()));
    }

    // A hash of the plugin in the flags makes kbuild rebuild the kernel
    // when it changes.
    let id = format!("{:016x}", fnv64(&std::fs::read("plugin/ksee-trace.so").unwrap_or_default()));
    let plugin = Path::new("plugin/ksee-trace.so").canonicalize().unwrap();
    // (Inlining only what the source asks for, `inline`: the compiler's
    // own choices would hide calls, with their arguments, as inlined code.)
    let kmake = |args: &[&str]| {
        let mut c = Command::new("make");
        c.args(["-s", "-C", "linux", "ARCH=wasm", &format!("LLVM={llvm}"),
                &format!("KCFLAGS=-g -finline-hint-functions -fpass-plugin={} -DKSEE_ID={id}", plugin.display())])
            .args(args);
        c
    };
    if stale("linux/.config", &["linux/arch/wasm/configs/defconfig"]) {
        run(&mut kmake(&["defconfig"]));
    }
    eprintln!("  KERNEL  linux/vmlinux");
    run(&mut kmake(&[&format!("-j{}", std::thread::available_parallelism().map_or(4, |n| n.get()))]));
    // (Its UAPI headers: the pack's constants, the programs' syscalls.)
    run(&mut kmake(&["headers"]));

    site();
    // (The runtime and the page, one module: the page calls the Machine.)
    eprintln!("  RUNTIME {SITE}/pkg");
    run(Command::new("cargo").args(["build", "-q", "--release", "-p", "ksee-page",
        "--target", "wasm32-unknown-unknown"]));
    run(Command::new("wasm-bindgen").args(["--target", "web", "--no-typescript", "--out-dir", &format!("{SITE}/pkg"),
        "target/wasm32-unknown-unknown/release/ksee_page.wasm"]));

    // (Packed again for a new kernel, or a new pack: this program.)
    let me = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
    if stale("target/vmlinux", &["linux/vmlinux", &me]) {
        crate::pack::pack(Path::new("linux"), Path::new(SITE));
        std::fs::rename(format!("{SITE}/vmlinux"), "target/vmlinux")
            .unwrap_or_else(|e| crate::fail(&format!("target/vmlinux: {e}")));
        gzip("target/vmlinux", &format!("{SITE}/vmlinux.gz"));
    }
    // The programs (/init, /bin/prog): plain C, no C library (their syscalls:
    // user/syscall.h), with the kernel's UAPI headers.
    let programs = ["init", "prog"];
    let inputs = ["user/init.c", "user/prog.c", "user/syscall.h", "user/initramfs.list", "linux/usr/gen_init_cpio"];
    let dir = "target/initramfs";
    let cpio_out = format!("{dir}/initramfs.cpio");
    if stale(&cpio_out, &inputs) {
        eprintln!("  INIT    {SITE}/initramfs.cpio.gz");
        std::fs::create_dir_all(dir).unwrap_or_else(|e| crate::fail(&format!("{dir}: {e}")));
        for p in programs {
            run(Command::new(format!("clang{llvm}"))
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
        run(Command::new("mkfs.ext4").args(["-q", "-F", "-b", "1024", "-L", "ksee", "-N", "32", "-d", &files,
                                            "-E", "lazy_itable_init=0,lazy_journal_init=0", &disk, "2M"]));
        // (The list's paths are relative to the dir.)
        let tool = Path::new("linux/usr/gen_init_cpio").canonicalize()
            .unwrap_or_else(|e| crate::fail(&format!("linux/usr/gen_init_cpio: {e}")));
        let cpio = Command::new(tool).arg(Path::new("user/initramfs.list").canonicalize().unwrap()).current_dir(dir).output()
            .unwrap_or_else(|e| crate::fail(&format!("gen_init_cpio: {e}")));
        if !cpio.status.success() {
            crate::fail(&format!("gen_init_cpio: {}", String::from_utf8_lossy(&cpio.stderr)));
        }
        std::fs::write(&cpio_out, cpio.stdout).unwrap_or_else(|e| crate::fail(&format!("{cpio_out}: {e}")));
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
/// linux.patch applied, the first time; then linux.patch, written from it
/// again when it changes: the port, all of it (the files new too) as it
/// differs from the release.
fn linux() {
    let git = |args: &[&str]| {
        let out = Command::new("git").arg("-C").arg("linux").args(args).output()
            .unwrap_or_else(|e| crate::fail(&format!("git: {e}")));
        if !out.status.success() {
            crate::fail(&format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr)));
        }
        out.stdout
    };
    if !Path::new("linux/Makefile").exists() {
        eprintln!("  LINUX   linux/ ({LINUX_TAG}, from the stable tree; linux.patch)");
        std::fs::create_dir_all("linux").unwrap_or_else(|e| crate::fail(&format!("linux/: {e}")));
        git(&["init", "-q"]);
        git(&["fetch", "-q", "--depth=1", LINUX_TREE, "tag", LINUX_TAG]);
        let commit = git(&["rev-parse", &format!("{LINUX_TAG}^{{commit}}")]);
        if String::from_utf8_lossy(&commit).trim() != LINUX_COMMIT {
            crate::fail(&format!("{LINUX_TAG}: not {LINUX_COMMIT}"));
        }
        git(&["checkout", "-q", LINUX_TAG]);
        git(&["apply", "../linux.patch"]);
        return;
    }
    git(&["add", "-A", "--intent-to-add"]);
    let diff = git(&["diff", "--no-color", LINUX_TAG]);
    if std::fs::read("linux.patch").ok().as_deref() != Some(&diff[..]) {
        eprintln!("  PATCH   linux.patch");
        std::fs::write("linux.patch", &diff).unwrap_or_else(|e| crate::fail(&format!("linux.patch: {e}")));
    }
}

fn gzip(from: &str, to: &str) {
    let out = std::fs::File::create(to).unwrap_or_else(|e| crate::fail(&format!("{to}: {e}")));
    run(Command::new("gzip").args(["-9nc", from]).stdout(out));
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

/// Build, then: core's unit tests, the page's (wasm, under Node:
/// wasm-bindgen-test-runner), and under Deno tests/web.js (the kernel, its
/// recordings, every example: the page's module built with its tests,
/// crates/ksee-page's selftest, in target/test/); with `ui`, also
/// tests/ui.js (the page in headless Chromium, $CHROME, running its own
/// tests, crates/ksee-page/src/uitest.rs: the read example, a changed
/// program, the pipe example, a phone's screen).
pub fn test(ui: bool) {
    build();
    run(Command::new("cargo").args(["test", "-q", "-p", "ksee-core"]));
    // (Not -q: the runner takes no flags.)
    run(Command::new("cargo").args(["test", "-p", "ksee-page", "--target", "wasm32-unknown-unknown"])
        .env("CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER", "wasm-bindgen-test-runner"));
    // (Its own target directory: the site's module, built without them, stays as it is.)
    run(Command::new("cargo").args(["build", "-q", "--release", "-p", "ksee-page", "--features", "selftest",
        "--target", "wasm32-unknown-unknown", "--target-dir", "target/test/build"]));
    run(Command::new("wasm-bindgen").args(["--target", "web", "--no-typescript", "--out-dir", "target/test/pkg",
        "target/test/build/wasm32-unknown-unknown/release/ksee_page.wasm"]));
    std::fs::copy("web/ksee.js", "target/test/ksee.js").unwrap_or_else(|e| crate::fail(&format!("target/test/ksee.js: {e}")));
    run(Command::new("deno").args(["run", "--allow-read", "tests/web.js"]));
    if ui {
        // (The site with the page's tests: the site's files, its module the one built with them.)
        let site = Path::new("target/test/site");
        let _ = std::fs::remove_dir_all(site);
        std::fs::create_dir_all(site).unwrap_or_else(|e| crate::fail(&format!("{}: {e}", site.display())));
        for e in std::fs::read_dir("target/site").into_iter().flatten().flatten() {
            let name = e.file_name();
            let from = if name == "pkg" { Path::new("../pkg").to_path_buf() } else { Path::new("../../site").join(&name) };
            std::os::unix::fs::symlink(from, site.join(&name)).unwrap_or_else(|e| crate::fail(&format!("{}: {e}", site.display())));
        }
        for test in ["read", "back", "pipe", "phone"] {
            run(Command::new("deno").args(["run", "-A", "tests/ui.js", &format!("target/ui-{test}.png"), &format!("--test={test}")]));
        }
    }
}

fn fnv64(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf29ce484222325, |h, &b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}
