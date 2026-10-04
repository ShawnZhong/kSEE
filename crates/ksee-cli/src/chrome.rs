//! `ksee test` in Chrome's headless shell (target/chrome/, the Makefile's version),
//! without its debugging protocol: the page under `#test=NAME` runs its test
//! and POSTs `ok` or `fail` and its log to the server's /result.

use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::channel;
use std::time::Duration;

use crate::or_fail;

/// The tests, each the page it opens and the screen it has: the URL's
/// lesson page and hash (`test=NAME` runs it, and it POSTs `ok` or `fail`
/// and its log to /result; history's program, changed, raw: its JSON has no
/// & or %). The kernel's suite is ksee-host's; the rest, ksee-page's.
pub const TESTS: &[(&str, &str, (u32, u32))] = &[
    ("kernel", "#test=kernel", SCREEN),
    ("lessons", "#test=lessons", SCREEN),
    ("read", "read.html#test=read", SCREEN),
    ("history", r#"write.html#p=["int fd = openat(AT_FDCWD, \"/note.txt\", O_WRONLY | O_APPEND)","write(fd, \"changed;\\n\", 9)"]&test=history"#, SCREEN),
    ("tasks", "pipe.html#test=tasks", SCREEN),
    ("responsive", "read.html#test=responsive", (390, 844)),
];
const SCREEN: (u32, u32) = (1500, 900);

/// The browser: `make chrome`'s.
const BROWSER: &str = "target/chrome/chrome-headless-shell";

/// How long a test may take (the kernel's take minutes).
const TIMEOUT: Duration = Duration::from_secs(300);

/// Test `name` (on the site, target/site, in the headless shell): whether the page said `ok` (its log printed either way).
pub fn test(name: &str) -> bool {
    let &(_, hash, (width, height)) = TESTS.iter().find(|t| t.0 == name).expect("a test's name");
    let listener = or_fail(TcpListener::bind("127.0.0.1:0"), "serve");
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    let (tx, rx) = channel();
    std::thread::spawn(move || crate::serve::serve_on(listener, Path::new(crate::build::SITE), tx));
    let profile = std::env::temp_dir().join(format!("ksee-chrome-{}", std::process::id()));
    let mut child = or_fail(Command::new(BROWSER)
        .args(["--headless=new", "--no-sandbox", "--no-first-run", &format!("--user-data-dir={}", profile.display()),
               &format!("--window-size={width},{height}"), &format!("http://127.0.0.1:{port}/{hash}")])
        .stdout(Stdio::null()).stderr(Stdio::null()).spawn(), BROWSER);
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
