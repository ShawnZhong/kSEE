//! `ksee test` and `ksee shot` in headless Chromium ($CHROME), without its
//! debugging protocol: the page under `#test=NAME` runs its test and POSTs
//! `ok` or `fail` and its log to the server's /result; screenshots are
//! Chrome's own --screenshot.

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::channel;
use std::time::Duration;

use crate::fail;

/// The tests (crates/ksee-page's uitest.rs), each with the page it opens
/// (its URL's hash: the lesson, and `test`, which runs the test, POSTing
/// `ok` or `fail` and its log to /result) and the screen it has: the
/// kernel's, then the page's: the read example, the write example with its
/// program changed in the URL, the pipe example, the read example on a
/// phone's screen.
pub const TESTS: &[(&str, &str, (u32, u32))] = &[("kernel", "#test=kernel", SCREEN), ("read", "#ex=read&test=read", SCREEN), ("back", "", SCREEN),
                                                  ("pipe", "#ex=pipe&test=pipe", SCREEN), ("phone", "#ex=read&test=phone", (390, 844))];
const SCREEN: (u32, u32) = (1500, 900);
// (Raw in the URL: the value holds no & or %; the page splits at the first = and decodes the rest.)
const BACK: &str = r#"int fd = openat(AT_FDCWD, "/note.txt", O_WRONLY | O_APPEND); write(fd, "changed;\n", 9)"#;

/// Chromium ($CHROME), headless, with `args`; and its profile dir, for the
/// caller to remove once it is gone.
fn launch(args: &[String]) -> (Child, PathBuf) {
    let bin = std::env::var("CHROME").unwrap_or_else(|_| fail("set CHROME to a (headless) Chromium binary"));
    let dir = std::env::temp_dir().join(format!("ksee-chrome-{}", std::process::id()));
    let child = Command::new(bin).args(["--headless=new", "--no-sandbox", "--no-first-run",
            &format!("--user-data-dir={}", dir.display())]).args(args)
        .stdout(Stdio::null()).stderr(Stdio::null()).spawn()
        .unwrap_or_else(|e| fail(&format!("$CHROME: {e}")));
    (child, dir)
}

/// Gone: killed (it lingers after its screenshot), its profile removed.
fn reap(mut child: Child, profile: &Path) {
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(profile);
}

/// Whether `out` has appeared (non-empty) since `start`.
fn fresh(out: &Path, start: std::time::SystemTime) -> bool {
    std::fs::metadata(out).is_ok_and(|m| m.len() > 0 && m.modified().is_ok_and(|t| t > start))
}

/// Until `out` is fresh, `secs` at most.
fn await_shot(out: &Path, start: std::time::SystemTime, secs: u64) -> bool {
    while !fresh(out, start) && start.elapsed().unwrap_or(Duration::ZERO) < Duration::from_secs(secs) {
        std::thread::sleep(Duration::from_millis(200));
    }
    fresh(out, start)
}

/// Test `name` (on the site built with them, target/test/site): whether the
/// page said `ok` (its log printed either way); a screenshot in `out`, if
/// one is given (Chrome takes it once settled: the test's virtual-time
/// budget).
pub fn test(name: &str, out: Option<PathBuf>) -> bool {
    let Some(&(_, hash, size)) = TESTS.iter().find(|t| t.0 == name) else {
        fail(&format!("no test {name}: {}", TESTS.iter().map(|t| t.0).collect::<Vec<_>>().join(", ")))
    };
    let hash = if name == "back" { format!("#ex=write&p={BACK}&test=back") } else { hash.into() };
    // (The kernel's tests take minutes; the page's, two at most.)
    let timeout = Duration::from_secs(if name == "kernel" { 300 } else { 120 });
    let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| fail(&format!("serve: {e}")));
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    let (tx, rx) = channel();
    std::thread::spawn(move || crate::serve::serve_on(listener, Path::new("target/test/site"), Some(tx)));
    let start = std::time::SystemTime::now();
    let mut args = vec![format!("--window-size={},{}", size.0, size.1),
        format!("--virtual-time-budget={}", timeout.as_millis()), format!("http://127.0.0.1:{port}/{hash}")];
    if let Some(out) = &out {
        args.insert(0, format!("--screenshot={}", out.display()));
    }
    let (child, profile) = launch(&args);
    let outcome = rx.recv_timeout(timeout);
    let passed = outcome.map(|o| {
        print!("{log}", log = o.log);
        o.passed
    }).unwrap_or(false);
    if !passed {
        reap(child, &profile);
        println!("{name}: FAIL");
        return false;
    }
    // (Its screenshot, if one: taken once settled, Chrome lingering after.)
    let shot = out.is_none_or(|out| await_shot(&out, start, 120));
    reap(child, &profile);
    println!("{name}: {}", if passed && shot { "PASS" } else { "FAIL" });
    passed && shot
}

/// What to look at: the site at `hash` (or `url`), a screen `size`, settled
/// after `wait` ms of virtual time, as a screenshot in `out`.
pub struct Shot {
    pub out: PathBuf,
    pub hash: String,
    pub size: (u32, u32),
    pub wait: u64,
    pub url: Option<String>,
}

impl Shot {
    pub fn run(&self) -> bool {
        let url = self.url.clone().unwrap_or_else(|| {
            // (Served here, on a port of its own.)
            let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| fail(&format!("serve: {e}")));
            let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
            std::thread::spawn(move || crate::serve::serve_on(listener, Path::new(crate::build::SITE), None));
            format!("http://127.0.0.1:{port}/{}", self.hash)
        });
        let start = std::time::SystemTime::now();
        let (child, profile) = launch(&[format!("--screenshot={}", self.out.display()),
            format!("--window-size={},{}", self.size.0, self.size.1),
            format!("--virtual-time-budget={}", self.wait), url]);
        // (Taken once settled, Chrome lingering after.)
        let ok = await_shot(&self.out, start, self.wait / 1000 + 120);
        reap(child, &profile);
        println!("{}", if ok { format!("saved {}", self.out.display()) } else { "no screenshot".into() });
        ok
    }
}
