//! `ksee build` and `ksee test`: the build is the Makefile's (`make site`:
//! the kernel's half, then the site, target/site, its steps that are code
//! this command's: pack, lessons, bindgen).
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

/// `make` (the Makefile's) for `args`, its jobs in parallel; this
/// command, its steps'.
fn make(args: &[&str]) {
    let jobs = format!("-j{}", std::thread::available_parallelism().map_or(4, |n| n.get()));
    let me = std::env::current_exe().map(|p| format!("KSEE={}", p.display())).unwrap_or_default();
    run(Command::new("make").args(["--no-print-directory", &jobs, &me]).args(args));
}

pub fn build() {
    make(&["site"]);
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

/// linux.patch, written from linux/ (the Makefile's), never by the build.
pub fn patch() {
    make(&["patch"]);
}

/// wasm-bindgen's glue for `wasm`, in `out` (cli-support, at ksee-page's
/// wasm-bindgen version: the two must match).
pub fn bindgen(wasm: &str, out: &str) {
    eprintln!("+ (wasm-bindgen) {wasm} -> {out}");
    wasm_bindgen_cli_support::Bindgen::new()
        .input_path(wasm)
        .web(true)
        .and_then(|b| b.typescript(false).generate(out))
        .unwrap_or_else(|e| crate::fail(&format!("wasm-bindgen: {e:#}")));
}

/// Run native tests, build (`built`: an earlier CI step did), and run
/// `names` on the site in the headless shell (target/chrome/).
pub fn test(names: &[String], built: bool) {
    run(Command::new("cargo").args(["test", "--locked", "--release", "-p", "ksee-core", "-p", "ksee-cli"]));
    if !built {
        build();
    }
    // (The browser, the Makefile's version: fetched the first time.)
    make(&["chrome"]);
    for name in names {
        if !crate::chrome::test(name) {
            crate::fail(&format!("the {name} test failed"));
        }
    }
}
