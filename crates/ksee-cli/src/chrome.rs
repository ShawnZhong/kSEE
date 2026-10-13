//! `ksee test` in Chrome's headless shell (target/chrome/, setup.sh's),
//! without its debugging protocol: the page under `#test=NAME` runs its test
//! and says `ok` or `fail` and its log on the console, which the shell
//! writes on its stderr; there an error nothing caught fails it at once,
//! the page's own code run or not.

use std::io::BufRead;
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::channel;
use std::time::Duration;

use crate::or_fail;

/// The tests, each the page it opens and the screen it has: the URL's
/// lesson page and hash (`test=NAME` runs it; url's program, changed,
/// raw: its JSON has no & or %). The kernel's suite is ksee-host's; the rest, ksee-page's.
pub const TESTS: &[(&str, &str, (u32, u32))] = &[
    ("kernel", "#test=kernel", SCREEN),
    ("lessons", "#test=lessons", SCREEN),
    ("read", "read.html#test=read", SCREEN),
    ("url", r#"write.html#p=["int fd = openat(AT_FDCWD, \"/note.txt\", O_WRONLY | O_APPEND)","write(fd, \"changed;\\n\", 9)"]&test=url"#, SCREEN),
    ("tasks", "pipe.html#test=tasks", SCREEN),
    ("disk", "durable.html#test=disk", SCREEN),
    ("responsive", "read.html#test=responsive", (390, 844)),
    ("tour", "#test=tour", SCREEN),
];
const SCREEN: (u32, u32) = (1500, 900);

/// The browser: setup.sh's.
pub const BROWSER: &str = "target/chrome/chrome-headless-shell";

/// How long a test may take (the longest, seconds: the kernel's).
const TIMEOUT: Duration = Duration::from_secs(60);

/// Test `name` (on the site, target/site, in the headless shell): whether the page said `ok` (its log printed either way).
pub fn test(name: &str) -> bool {
    let &(_, hash, (width, height)) = TESTS.iter().find(|t| t.0 == name).expect("a test's name");
    let listener = or_fail(TcpListener::bind("127.0.0.1:0"), "serve");
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    let (tx, rx) = channel();
    std::thread::spawn(move || crate::serve::serve_on(listener, Path::new(crate::build::SITE)));
    let profile = std::env::temp_dir().join(format!("ksee-chrome-{}", std::process::id()));
    let mut child = or_fail(Command::new(BROWSER)
        .args(["--headless=new", "--no-sandbox", "--no-first-run", "--enable-logging=stderr", "--v=0", &format!("--user-data-dir={}", profile.display()),
               &format!("--window-size={width},{height}"), &format!("http://127.0.0.1:{port}/{hash}")])
        .stdout(Stdio::null()).stderr(Stdio::piped()).spawn(), BROWSER);
    // (The console's lines: the test's outcome; "Uncaught …", a failure.)
    if let Some(err) = child.stderr.take() {
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(err).lines().map_while(Result::ok) {
                let outcome = if let Some((_, said)) = line.split_once("\"ksee test: ") {
                    // ("ok "LOG"", source: …: the log a JSON string.)
                    let (verdict, log) = said.split_once(' ').unwrap_or((said, ""));
                    let log = log.rsplit_once("\", source: ").map_or(log, |l| l.0);
                    (verdict == "ok", serde_json::from_str::<String>(log).unwrap_or_else(|_| log.to_string()))
                } else if let Some((_, said)) = line.split_once("\"Uncaught ") {
                    let (what, from) = said.split_once("\", source: ").unwrap_or((said, ""));
                    (false, format!("uncaught {what} ({})", from.trim_end()))
                } else {
                    continue;
                };
                let _ = tx.send(outcome);
            }
        });
    }
    // (Nothing said by then: stuck, a kernel that hangs.)
    let (passed, log) = rx.recv_timeout(TIMEOUT)
        .unwrap_or_else(|_| (false, format!("{name}: nothing said in {} s (stuck?)", TIMEOUT.as_secs())));
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&profile);
    if !log.is_empty() {
        println!("{}", log.trim_end());
    }
    println!("{name}: {}", if passed { "PASS" } else { "FAIL" });
    passed
}
