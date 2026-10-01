//! `ksee test` in headless Chromium ($CHROME), without its debugging
//! protocol: the page under `#test=NAME` runs its test and POSTs `ok` or
//! `fail` and its log to the server's /result.

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

/// Gone: killed, its profile removed.
fn reap(mut child: Child, profile: &Path) {
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(profile);
}

/// How long a test may take: the kernel's take minutes, the page's two at most.
const KERNEL_TIMEOUT: u64 = 300;
const UI_TIMEOUT: u64 = 120;

/// Test `name` (on the site built with them, target/test/site): whether the
/// page said `ok` (its log printed either way).
pub fn test(name: &str) -> bool {
    let Some(&(_, hash, size)) = TESTS.iter().find(|t| t.0 == name) else {
        fail(&format!("no test {name}: {}", TESTS.iter().map(|t| t.0).collect::<Vec<_>>().join(", ")))
    };
    let hash = if name == "back" { format!("#ex=write&p={BACK}&test=back") } else { hash.into() };
    let timeout = Duration::from_secs(if name == "kernel" { KERNEL_TIMEOUT } else { UI_TIMEOUT });
    let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| fail(&format!("serve: {e}")));
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    let (tx, rx) = channel();
    std::thread::spawn(move || crate::serve::serve_on(listener, Path::new("target/test/site"), Some(tx)));
    let args = vec![format!("--window-size={},{}", size.0, size.1),
        format!("http://127.0.0.1:{port}/{hash}")];
    let (child, profile) = launch(&args);
    let outcome = rx.recv_timeout(timeout);
    let passed = outcome.map(|o| {
        print!("{log}", log = o.log);
        o.passed
    }).unwrap_or(false);
    reap(child, &profile);
    println!("{name}: {}", if passed { "PASS" } else { "FAIL" });
    passed
}
