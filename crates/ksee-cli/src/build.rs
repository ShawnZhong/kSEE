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
//! The tools are setup.sh's (LLVM with lld's wasm-ld, GNU make, mkfs.ext4):
//! found once by tools(), called by absolute path. Children that run their
//! own tools (kbuild, clang's wasm link) get the dirs on their PATH.
use std::path::Path;
use std::process::Command;

/// `cmd` as a shell line: its program and arguments (quoted only past
/// [A-Za-z0-9_./=:,+@%-]), `cd DIR &&` first when it runs elsewhere, and any
/// environment set on it as `K=V`.
fn shell(cmd: &Command) -> String {
    let mut s = String::new();
    for (k, v) in cmd.get_envs() {
        if let Some(v) = v {
            s.push_str(&word(&k.to_string_lossy()));
            s.push('=');
            s.push_str(&word(&v.to_string_lossy()));
            s.push(' ');
        }
    }
    if let Some(dir) = cmd.get_current_dir() {
        s.push_str("cd ");
        s.push_str(&word(&dir.to_string_lossy()));
        s.push_str(" && ");
    }
    s.push_str(&word(&cmd.get_program().to_string_lossy()));
    for a in cmd.get_args() {
        s.push(' ');
        s.push_str(&word(&a.to_string_lossy()));
    }
    s
}

/// A shell word, quoted only past [A-Za-z0-9_./=:,+@%-].
fn word(w: &str) -> String {
    if !w.is_empty() && w.chars().all(|c| c.is_ascii_alphanumeric() || "./=:,+@%-_".contains(c)) {
        w.into()
    } else {
        format!("'{}'", w.replace('\'', "'\\''"))
    }
}

/// Runs `cmd`, said first, its output shown.
pub fn run(cmd: &mut Command) {
    eprintln!("+ {}", shell(cmd));
    let status = cmd.status().unwrap_or_else(|e| crate::fail(&format!("{}: {e}", shell(cmd))));
    if !status.success() {
        crate::fail(&format!("{} failed: {status}", shell(cmd)));
    }
}

/// Runs `cmd`, said first, for its stdout (its stderr shown, its stdout said
/// after when short, else its size).
fn output(cmd: &mut Command) -> Vec<u8> {
    eprintln!("+ {}", shell(cmd));
    let out = cmd.stderr(std::process::Stdio::inherit()).output()
        .unwrap_or_else(|e| crate::fail(&format!("{}: {e}", shell(cmd))));
    if !out.status.success() {
        crate::fail(&format!("{} failed: {}", shell(cmd), out.status));
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

/// setup.sh's tools (LLVM with lld's wasm-ld, GNU make, mkfs.ext4): their
/// paths, and the dirs for children that run their own tools (kbuild, clang's
/// wasm link). Homebrew's on macOS, apt.llvm.org's on Linux.
struct Tools {
    path: String,
    llvm_config: String,
    cc: String,
    cxx: String,
    make: String,
    mkfs: String,
}

fn tools() -> Tools {
    // The first of `names` in `dir` (setup.sh installs it).
    let tool = |dir: &str, names: &[&str]| names.iter().find_map(|n| {
        Path::new(dir).join(n).is_file().then(|| format!("{dir}/{n}"))
    }).unwrap_or_else(|| crate::fail(&format!("{dir}/{}: run setup.sh", names[0])));
    let (llvm, lld, make, mkfs) = if cfg!(target_os = "macos") {
        // (brew is at its default prefix; PATH lookup only as a fallback.)
        let brew = ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"].iter().find_map(|b| {
            Path::new(b).is_file().then(|| b.to_string())
        }).unwrap_or_else(|| "brew".into());
        let brew = String::from_utf8_lossy(&output(Command::new(&brew).arg("--prefix"))).trim().to_string();
        // (Homebrew's lld lands in its own dir, not llvm's bindir; make's
        // gnubin dir only when brewed with it, else gmake in bin/.)
        let dir_with = |dir: &str, name: &str| Path::new(&format!("{brew}/{dir}/{name}")).is_file().then(|| format!("{brew}/{dir}"));
        let lld = dir_with("opt/lld/bin", "wasm-ld").or_else(|| dir_with("bin", "wasm-ld"))
            .unwrap_or_else(|| crate::fail("wasm-ld: run setup.sh"));
        let (make_dir, make_name) = dir_with("opt/make/libexec/gnubin", "make").map(|d| (d, "make"))
            .or_else(|| dir_with("bin", "gmake").map(|d| (d, "gmake")))
            .unwrap_or_else(|| crate::fail("GNU make: run setup.sh"));
        let e2fs = format!("{brew}/opt/e2fsprogs/sbin");
        (format!("{brew}/opt/llvm/bin"), lld, format!("{make_dir}/{make_name}"), tool(&e2fs, &["mkfs.ext4"]))
    } else {
        // The newest /usr/lib/llvm-*/bin (the plain names in its dir).
        let mut best: Option<(u32, String)> = None;
        for e in std::fs::read_dir("/usr/lib").into_iter().flatten().flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if let Some(v) = n.strip_prefix("llvm-").and_then(|v| v.parse::<u32>().ok()) {
                let dir = format!("/usr/lib/{n}/bin");
                if Path::new(&dir).is_dir() && best.as_ref().is_none_or(|&(b, _)| v > b) {
                    best = Some((v, dir));
                }
            }
        }
        let llvm = best.map(|(_, d)| d).unwrap_or_else(|| crate::fail("no /usr/lib/llvm-*/bin: run setup.sh"));
        (llvm.clone(), llvm, "make".into(), "mkfs.ext4".into())
    };
    let path = format!("{}:{}:{}:{}:{}", lld, llvm, make.rsplit_once('/').map_or("", |(d, _)| d),
        mkfs.rsplit_once('/').map_or("", |(d, _)| d), std::env::var("PATH").unwrap_or_default());
    Tools { llvm_config: tool(&llvm, &["llvm-config"]), cc: tool(&llvm, &["clang"]), cxx: tool(&llvm, &["clang++"]),
        make, mkfs, path }
}

/// Whether `out` is missing or older than any of `inputs`.
fn stale(out: &str, inputs: &[&str]) -> bool {
    let time = |p: &str| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let Some(t) = time(out) else { return true };
    inputs.iter().any(|i| time(i).is_none_or(|ti| ti > t))
}

pub fn build() {
    let t = tools();
    linux();

    if stale("plugin/ksee-trace.so", &["plugin/ksee-trace.cpp"]) {
        let flags = output(Command::new(&t.llvm_config).arg("--cxxflags"));
        let mut plugin = Command::new(&t.cxx);
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
    // (kbuild runs its own tools — clang, wasm-ld, llvm-ar — off PATH.)
    let jobs = format!("-j{}", std::thread::available_parallelism().map_or(4, |n| n.get()));
    let kmake = |args: &[&str]| {
        let mut c = Command::new(&t.make);
        c.env("PATH", &t.path);
        c.args(["-C", "linux", "ARCH=wasm", "LLVM=1",
                &format!("KCFLAGS=-g -finline-hint-functions -fpass-plugin={} -DKSEE_ID={id}", plugin.display())])
            .arg(&jobs)
            .args(args);
        c
    };
    if stale("linux/.config", &["linux/arch/wasm/configs/defconfig"]) {
        run(&mut kmake(&["defconfig"]));
    }
    run(&mut kmake(&[]));
    // (Its UAPI headers: the pack's constants, the programs' syscalls.)
    run(&mut kmake(&["headers"]));

    site();
    // (The runtime and the page, one module: the page calls the Machine.)
    run(Command::new("cargo").args(["build", "--release", "-p", "ksee-page",
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
    let inputs = ["user/init.c", "user/prog.c", "user/syscall.h", "user/initramfs.list", "linux/usr/gen_init_cpio"];
    let dir = "target/initramfs";
    let cpio_out = format!("{dir}/initramfs.cpio");
    if stale(&cpio_out, &inputs) {
        std::fs::create_dir_all(dir).unwrap_or_else(|e| crate::fail(&format!("{dir}: {e}")));
        for p in programs {
            run(Command::new(&t.cc)
                // (Position-independent, in the kernel's memory: see binfmt_wasm.c.
                // clang finds wasm-ld off PATH.)
                .env("PATH", &t.path)
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
        run(Command::new(&t.mkfs).args(["-F", "-b", "1024", "-L", "ksee", "-N", "32", "-d", &files,
                                            "-E", "lazy_itable_init=0,lazy_journal_init=0", &disk, "2M"]));
        // (The list's paths are relative to the dir.)
        let tool = Path::new("linux/usr/gen_init_cpio").canonicalize()
            .unwrap_or_else(|e| crate::fail(&format!("linux/usr/gen_init_cpio: {e}")));
        let cpio = output(Command::new(tool).arg(Path::new("user/initramfs.list").canonicalize().unwrap()).current_dir(dir));
        std::fs::write(&cpio_out, cpio).unwrap_or_else(|e| crate::fail(&format!("{cpio_out}: {e}")));
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
    output(Command::new("git").arg("-C").arg("linux").args(args))
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
    run(Command::new("cargo").args(["test", "-p", "ksee-core", "-p", "ksee-cli"]));
    // (Its own target directory: the site's module, built without them, stays as it is.)
    run(Command::new("cargo").args(["build", "--release", "-p", "ksee-page", "--features", "selftest",
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
