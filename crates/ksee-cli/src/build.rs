//! `ksee build` and `ksee test`: the build, in order. The kernel's half
//! (the plugin, linux/, the initramfs) is the Makefile's; then, each step
//! asking the tool that knows its own dependencies (cargo) or checking
//! whether its output is older than its inputs, the site, in target/site
//! (what `ksee serve` serves and `ksee deploy` pushes):
//!   pkg/                   the runtime, and wasm-bindgen's glue (cargo)
//!   vmlinux.gz, src/       the kernel for the page (pack; target/vmlinux)
//!   initramfs.cpio.gz, disk.img.gz
//!                          the Makefile's target/initramfs/initramfs.cpio, disk.img
//!   lessons.json, NAME.html the lessons (user/lessons/*.json): the page's, and
//!                          a page of it each (see lessons.rs)
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
/// lessons and their pages.
fn assets() {
    let jobs = format!("-j{}", std::thread::available_parallelism().map_or(4, |n| n.get()));
    run(Command::new("make").args(["--no-print-directory", &jobs]));
    crate::or_fail(std::fs::create_dir_all(SITE), SITE);
    packed_kernel();
    for name in ["initramfs.cpio", "disk.img"] {
        let (from, to) = (format!("target/initramfs/{name}"), format!("{SITE}/{name}.gz"));
        if stale(&to, &[&from]) {
            gzip(&from, &to);
        }
    }
    crate::lessons::write();
}

/// `cargo` building the page, told what it is built from (the page says it):
/// the commit (+: with changes not committed) and where the source is,
/// GitHub's page of the origin remote; neither without git.
fn built(cargo: &mut Command) -> &mut Command {
    if let (Some(commit), Some(source)) = (commit(), source()) {
        cargo.env("KSEE_COMMIT", commit).env("KSEE_SOURCE", source);
    }
    cargo
}

/// The commit checked out, and + if it has changes not committed.
pub(crate) fn commit() -> Option<String> {
    let dirty = git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
    Some(git(&["rev-parse", "--short=12", "HEAD"])? + if dirty { "+" } else { "" })
}

/// git's output for `args`, if it ran.
pub(crate) fn git(args: &[&str]) -> Option<String> {
    Command::new("git").args(args).output().ok().filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// The source's page: the origin remote's on GitHub (https://github.com/OWNER/REPO).
pub(crate) fn source() -> Option<String> {
    Some(git(&["remote", "get-url", "origin"])?.trim_end_matches(".git").replacen("git@github.com:", "https://github.com/", 1))
}

/// Where `ksee deploy` publishes the site: the origin's GitHub Pages
/// (https://OWNER.github.io/REPO/; OWNER.github.io's own, at its root).
pub(crate) fn published() -> Option<String> {
    let source = source()?;
    let (owner, repo) = source.strip_prefix("https://github.com/")?.split_once('/')?;
    let host = format!("{}.github.io", owner.to_lowercase());
    Some(if repo.eq_ignore_ascii_case(&host) { format!("https://{host}/") } else { format!("https://{host}/{repo}/") })
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


/// Run native tests, build (`built`: an earlier CI step did), and run
/// `names` on the site in the headless shell (target/chrome/).
pub fn test(names: &[String], built: bool) {
    run(Command::new("cargo").args(["test", "--locked", "--release", "-p", "ksee-core", "-p", "ksee-cli"]));
    if !built {
        build();
    }
    // (The browser, the Makefile's version: fetched the first time.)
    run(Command::new("make").args(["--no-print-directory", "chrome"]));
    for name in names {
        if !crate::chrome::test(name) {
            crate::fail(&format!("the {name} test failed"));
        }
    }
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
