//! `ksee build` and `ksee test`: the build, in order. The kernel's half
//! (the plugin, linux/, the initramfs) is the Makefile's; then, each step
//! asking the tool that knows its own dependencies (cargo) or checking
//! whether its output is older than its inputs, the site, in target/site
//! (what `ksee serve` serves and `ksee deploy` pushes): links to web/'s
//! files (the page's own), and
//!   pkg/                   the runtime, and wasm-bindgen's glue (cargo)
//!   vmlinux.gz, src/       the kernel for the page (pack; target/vmlinux)
//!   initramfs.cpio.gz      the Makefile's target/initramfs/initramfs.cpio
//!   examples.json          the examples (user/examples/*.json)
//!
//! (Gzipped by the build: hosts compress them less, or not at all; the
//! page unpacks them.)
use std::path::Path;
use std::process::Command;

/// `cmd` as a line: its program and arguments, spaced.
fn line(cmd: &Command) -> String {
    std::iter::once(cmd.get_program()).chain(cmd.get_args())
        .map(|s| s.to_string_lossy()).collect::<Vec<_>>().join(" ")
}

/// Runs `cmd`, said first, its output shown.
pub fn run(cmd: &mut Command) {
    eprintln!("+ {}", line(cmd));
    let status = crate::or_fail(cmd.status(), line(cmd));
    if !status.success() {
        crate::fail(&format!("{} failed: {status}", line(cmd)));
    }
}

/// Runs `cmd`, said first, and returns its stdout; stderr remains visible.
pub(crate) fn stdout(cmd: &mut Command) -> Vec<u8> {
    eprintln!("+ {}", line(cmd));
    let out = crate::or_fail(cmd.stderr(std::process::Stdio::inherit()).output(), line(cmd));
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

pub fn build() {
    assets();
    page();
}

/// The site but its module: the kernel, its sources, the initramfs, the
/// examples, web/'s files.
fn assets() {
    let jobs = format!("-j{}", std::thread::available_parallelism().map_or(4, |n| n.get()));
    run(Command::new("make").args(["--no-print-directory", &jobs]));
    site();
    packed_kernel();
    let (cpio, compressed) = ("target/initramfs/initramfs.cpio", format!("{SITE}/initramfs.cpio.gz"));
    if stale(&compressed, &[cpio]) {
        gzip(cpio, &compressed);
    }
    examples();
}

/// `cargo` building the page, told what it is built from (the page says it):
/// the commit (+: with changes not committed) and where the source is,
/// GitHub's page of the origin remote; neither without git.
fn built(cargo: &mut Command) -> &mut Command {
    let git = |args: &[&str]| Command::new("git").args(args).output().ok().filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    if let (Some(commit), Some(remote)) = (git(&["rev-parse", "--short=12", "HEAD"]), git(&["remote", "get-url", "origin"])) {
        let dirty = git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
        let source = remote.trim_end_matches(".git").replacen("git@github.com:", "https://github.com/", 1);
        cargo.env("KSEE_COMMIT", format!("{commit}{}", if dirty { "+" } else { "" })).env("KSEE_SOURCE", source);
    }
    cargo
}

/// pkg/: the runtime and the page, one module (the page calls the Machine).
fn page() {
    run(built(Command::new("cargo").args(["build", "--locked", "--release", "-p", "ksee-page",
        "--target", "wasm32-unknown-unknown"])));
    bindgen("target/wasm32-unknown-unknown/release/ksee_page.wasm", &format!("{SITE}/pkg"));
}

/// The kernel for the page: target/vmlinux and the sources (pack), and
/// vmlinux.gz.
fn packed_kernel() {
    // (Packed again for a new kernel or pack implementation, or to restore
    // either required result after a partial clean.)
    let me = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
    let pack_inputs = ["linux/vmlinux", me.as_str()];
    let sources = format!("{SITE}/src");
    if stale("target/vmlinux", &pack_inputs) || stale(&sources, &pack_inputs) {
        eprintln!("+ (pack) linux -> target/vmlinux, {sources}");
        crate::pack::kernel(Path::new("linux"), Path::new("target/vmlinux"), Path::new(&sources));
    }
    let packed = format!("{SITE}/vmlinux.gz");
    if stale(&packed, &["target/vmlinux"]) {
        gzip("target/vmlinux", &packed);
    }
}

/// linux.patch, written from linux/ (the Makefile's), never by the build.
pub fn patch() {
    run(Command::new("make").args(["--no-print-directory", "patch"]));
}

/// wasm-bindgen's glue for `wasm`, in `out` (cli-support, at ksee-page's
/// wasm-bindgen version: the two must match).
fn bindgen(wasm: &str, out: &str) {
    eprintln!("+ (wasm-bindgen) {wasm} -> {out}");
    wasm_bindgen_cli_support::Bindgen::new()
        .input_path(wasm)
        .web(true)
        .and_then(|b| b.typescript(false).generate(out))
        .unwrap_or_else(|e| crate::fail(&format!("wasm-bindgen: {e:#}")));
}

fn gzip(from: &str, to: &str) {
    let out = crate::or_fail(std::fs::File::create(to), to);
    run(Command::new("gzip").args(["-9nc", from]).stdout(out));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_outputs_are_detected() {
        let scratch = crate::Scratch::new("stale");
        let dir = &scratch.0;
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

/// examples.json: the examples (user/examples/NAME.json: each a list
/// of syscalls for /init to make, one at a time), in the course's order,
/// with their names and chapters (`group`). (A file not in the course is
/// left out.)
fn examples() {
    eprintln!("+ (examples) user/examples/*.json -> {SITE}/examples.json");
    let index: Vec<String> = EXAMPLES.iter().flat_map(|&(group, names)| names.iter().map(move |&name| {
        let text = String::from_utf8_lossy(&crate::read(Path::new(&format!("user/examples/{name}.json")))).into_owned();
        format!("  {{ \"name\": \"{name}\", \"group\": \"{group}\", {}", text.trim().strip_prefix('{').unwrap_or("}").trim_start())
    })).collect();
    let out = format!("{SITE}/examples.json");
    crate::or_fail(std::fs::write(&out, format!("[\n{}\n]\n", index.join(",\n"))), &out);
}

/// Links in the site to web/'s files (so an edit there needs no build), and
/// none to files no longer there.
fn site() {
    eprintln!("+ (site) web/* -> {SITE}/");
    crate::or_fail(std::fs::create_dir_all(SITE), SITE);
    for e in std::fs::read_dir(SITE).into_iter().flatten().flatten() {
        if e.path().is_symlink() && !e.path().exists() {
            let _ = std::fs::remove_file(e.path());
        }
    }
    for e in std::fs::read_dir("web").into_iter().flatten().flatten() {
        let link = Path::new(SITE).join(e.file_name());
        if !link.is_symlink() {
            crate::or_fail(std::os::unix::fs::symlink(Path::new("../../web").join(e.file_name()), &link), link.display());
        }
    }
}

/// Run native tests, build the site but its module (`built`: an earlier
/// CI step did), then the test page, and run `names` in headless Chromium.
pub fn test(names: &[String], built: bool) {
    run(Command::new("cargo").args(["test", "--locked", "--release", "-p", "ksee-core", "-p", "ksee-cli"]));
    if !built {
        assets();
    }
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
    run(built(Command::new("cargo").args(["build", "--locked", "--release", "-p", "ksee-page", "--features", "browser-tests",
        "--target", "wasm32-unknown-unknown", "--target-dir", "target/test/build"])));
    bindgen("target/test/build/wasm32-unknown-unknown/release/ksee_page.wasm", "target/test/pkg");
    // (The site with the tests: the site's files, its module the one built with them.)
    let site = Path::new("target/test/site");
    let _ = std::fs::remove_dir_all(site);
    crate::or_fail(std::fs::create_dir_all(site), site.display());
    let names = std::fs::read_dir(SITE).into_iter().flatten().flatten().map(|e| e.file_name()).filter(|n| n != "pkg");
    for (from, name) in names.map(|n| (Path::new("../../site").join(&n), n)).chain([(Path::new("../pkg").into(), "pkg".into())]) {
        crate::or_fail(std::os::unix::fs::symlink(from, site.join(&name)), site.display());
    }
}
