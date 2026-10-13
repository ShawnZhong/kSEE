//! `ksee test` in Google Chrome (`google-chrome`, headless), driven through
//! the DevTools protocol on a pipe: a page of the test's size (Chrome's
//! window, headless too, keeps a frame: it would not be the size asked
//! for), opened under `#test=NAME`, runs its test and says `ok` or `fail`
//! and its log on the console; an error nothing caught fails it at once,
//! the page's own code run or not.

use std::io::{BufRead, Write};
use std::sync::mpsc::Sender;
use std::net::TcpListener;
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::channel;
use std::time::Duration;

use serde_json::{json, Value};

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

const BROWSER: &str = "google-chrome";

/// How long a test may take (the longest, seconds: the kernel's).
const TIMEOUT: Duration = Duration::from_secs(60);

/// Test `name` (on the site, target/site, in Chrome): whether the page said `ok` (its log printed either way).
pub fn test(name: &str) -> bool {
    let &(_, hash, (width, height)) = TESTS.iter().find(|t| t.0 == name).expect("a test's name");
    let listener = or_fail(TcpListener::bind("127.0.0.1:0"), "serve");
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    let (tx, rx) = channel();
    std::thread::spawn(move || crate::serve::serve_on(listener, Path::new(crate::build::SITE)));
    let profile = std::env::temp_dir().join(format!("ksee-chrome-{}", std::process::id()));
    // (The protocol's pipe: Chrome reads on fd 3, writes on fd 4.)
    let ((commands, to_chrome), (from_chrome, replies)) = (or_fail(std::io::pipe(), "pipe"), or_fail(std::io::pipe(), "pipe"));
    let (read, write) = (commands.as_raw_fd(), replies.as_raw_fd());
    let mut cmd = Command::new(BROWSER);
    cmd.args(["--headless=new", "--no-sandbox", "--no-first-run", "--remote-debugging-pipe", &format!("--user-data-dir={}", profile.display())])
        .stdout(Stdio::null()).stderr(Stdio::null());
    // (After fork, before exec: only dup2, async-signal-safe.)
    unsafe {
        cmd.pre_exec(move || if libc::dup2(read, 3) < 0 || libc::dup2(write, 4) < 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) });
    }
    let mut child = or_fail(cmd.spawn(), format!("{BROWSER} (./setup.sh installs it)"));
    drop((commands, replies));
    let mut chrome = Protocol { to: to_chrome, from: std::io::BufReader::new(from_chrome), next: 0 };
    // (A page of the test's size, then the test: the session open while it
    // runs, as the size lasts only as long.)
    match chrome.page(width, height, &format!("http://127.0.0.1:{port}/{hash}")) {
        Ok(()) => drop(std::thread::spawn(move || chrome.outcome(tx))),
        Err(e) => drop(tx.send((false, format!("{name}: Chrome: {e}")))),
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

/// The DevTools protocol, on Chrome's pipe: JSON messages, each ended by a NUL.
struct Protocol {
    to: std::io::PipeWriter,
    from: std::io::BufReader<std::io::PipeReader>,
    next: u64,
}

impl Protocol {
    /// Method `method` with `params` (in session `session`, a page's): its
    /// result (the events before it skipped).
    fn call(&mut self, session: Option<&str>, method: &str, params: Value) -> Result<Value, String> {
        self.next += 1;
        let mut m = json!({ "id": self.next, "method": method, "params": params });
        if let Some(s) = session {
            m["sessionId"] = json!(s);
        }
        self.to.write_all(format!("{m}\0").as_bytes()).map_err(|e| e.to_string())?;
        loop {
            let reply = self.message().ok_or("closed")?;
            if reply["id"] == self.next {
                return match reply.get("error") {
                    Some(e) => Err(format!("{method}: {e}")),
                    None => Ok(reply["result"].clone()),
                };
            }
        }
    }

    /// A new page, `width` by `height` (as a window of that size would
    /// show it), opened at `url`.
    fn page(&mut self, width: u32, height: u32, url: &str) -> Result<(), String> {
        let target = self.call(None, "Target.createTarget", json!({ "url": "about:blank" }))?;
        let session = self.call(None, "Target.attachToTarget", json!({ "targetId": target["targetId"], "flatten": true }))?;
        let session = session["sessionId"].as_str().ok_or("no session")?.to_string();
        self.call(Some(&session), "Emulation.setDeviceMetricsOverride",
                  json!({ "width": width, "height": height, "deviceScaleFactor": 1, "mobile": false }))?;
        self.call(Some(&session), "Runtime.enable", json!({}))?;
        self.call(Some(&session), "Page.navigate", json!({ "url": url }))?;
        Ok(())
    }

    /// The next message.
    fn message(&mut self) -> Option<Value> {
        let mut buf = Vec::new();
        (self.from.read_until(0, &mut buf).ok()? > 0).then_some(())?;
        buf.pop();
        serde_json::from_slice(&buf).ok()
    }

    /// The test's outcome, as the page's console says it ("ksee test: ok"
    /// or "fail", and its log), or an exception nothing caught: sent on
    /// `tx`.
    fn outcome(mut self, tx: Sender<(bool, String)>) {
        while let Some(m) = self.message() {
            let p = &m["params"];
            let outcome = match m["method"].as_str() {
                Some("Runtime.consoleAPICalled") => match p["args"][0]["value"].as_str() {
                    Some(said @ ("ksee test: ok" | "ksee test: fail")) => (said.ends_with("ok"), p["args"][1]["value"].as_str().unwrap_or_default().to_string()),
                    _ => continue,
                },
                Some("Runtime.exceptionThrown") => {
                    let e = &p["exceptionDetails"];
                    (false, format!("uncaught {} ({}:{})", e["exception"]["description"].as_str().or(e["text"].as_str()).unwrap_or_default(),
                                    e["url"].as_str().unwrap_or_default(), e["lineNumber"]))
                }
                _ => continue,
            };
            let _ = tx.send(outcome);
        }
    }
}
