//! `ksee test` in an auto-detected headless Chromium (`$CHROME` overrides),
//! without its debugging protocol: the page under `#test=NAME` runs its test
//! and POSTs `ok` or
//! `fail` and its log to the server's /result.

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::channel;
use std::time::Duration;

use crate::{fail, or_fail};

/// The tests (crates/ksee-page's test module), each with the page it opens
/// (its URL's hash: the lesson, and `test`, which runs the test, POSTing
/// `ok` or `fail` and its log to /result) and the screen it has: the
/// kernel's, then the page's: the read example, the write example with its
/// program changed in the URL (raw: the JSON holds no & or %; the page
/// decodes it), the pipe example, the read example on a phone's screen.
pub const TESTS: &[(&str, &str, (u32, u32))] = &[
    ("kernel", "#test=kernel", SCREEN),
    ("lesson", "#ex=read&test=lesson", SCREEN),
    ("history", r#"#ex=write&p=["int fd = openat(AT_FDCWD, \"/note.txt\", O_WRONLY | O_APPEND)","write(fd, \"changed;\\n\", 9)"]&test=history"#, SCREEN),
    ("tasks", "#ex=pipe&test=tasks", SCREEN),
    ("responsive", "#ex=read&test=responsive", (390, 844)),
];
const SCREEN: (u32, u32) = (1500, 900);

/// The requested Chromium, or one on PATH.
fn browser() -> PathBuf {
    if let Some(bin) = std::env::var_os("CHROME").filter(|p| !p.is_empty()) {
        return bin.into();
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            for name in ["google-chrome", "google-chrome-stable", "chromium", "chromium-browser", "chromium-headless-shell"] {
                let bin = dir.join(name);
                if bin.is_file() {
                    return bin;
                }
            }
        }
    }
    fail("no Chrome or Chromium found; set CHROME to its executable")
}

/// How long a test may take (the kernel's take minutes).
const TIMEOUT: Duration = Duration::from_secs(300);

/// Test `name` (on the site built with them, target/test/site, in a headless
/// Chromium): whether the page said `ok` (its log printed either way).
pub fn test(name: &str) -> bool {
    let &(_, hash, (width, height)) = TESTS.iter().find(|t| t.0 == name).expect("a test's name");
    let listener = or_fail(TcpListener::bind("127.0.0.1:0"), "serve");
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    let (tx, rx) = channel();
    std::thread::spawn(move || crate::serve::serve_on(listener, Path::new("target/test/site"), tx));
    let bin = browser();
    let profile = std::env::temp_dir().join(format!("ksee-chrome-{}", std::process::id()));
    let mut child = or_fail(Command::new(&bin)
        .args(["--headless=new", "--no-sandbox", "--no-first-run", &format!("--user-data-dir={}", profile.display()),
               &format!("--window-size={width},{height}"), &format!("http://127.0.0.1:{port}/{hash}")])
        .stdout(Stdio::null()).stderr(Stdio::null()).spawn(), bin.display());
    let (passed, log) = rx.recv_timeout(TIMEOUT).unwrap_or_default();
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&profile);
    if !log.is_empty() {
        println!("{}", log.trim_end());
    }
    println!("{name}: {}", if passed { "PASS" } else { "FAIL" });
    passed
}
