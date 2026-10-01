//! The page in headless Chromium ($CHROME), driven over its debugging pipe
//! (CDP: JSON messages, each ended by a NUL, it reading fd 3, writing fd
//! 4): `ksee shot` (a screenshot, after a script of one's own, to see the
//! page as a user would) and the tests (`ksee test`: each runs in the page
//! built with them, crates/ksee-page's uitest.rs).

use std::io::{BufRead, BufReader, PipeWriter, Write};
use std::net::TcpListener;
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::Duration;

use crate::fail;

unsafe extern "C" {
    fn dup2(old: i32, new: i32) -> i32;
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
}

/// A browser, a page in it attached; the exceptions the page threw so far.
pub struct Chrome {
    child: Child,
    to: PipeWriter,
    replies: Receiver<Json>,
    id: u64,
    session: Option<String>,
    exceptions: Arc<AtomicUsize>,
}

impl Chrome {
    /// Start it, a page `width` by `height` (px); its console and its
    /// exceptions printed as they come.
    pub fn start(width: u32, height: u32) -> Chrome {
        let bin = std::env::var("CHROME").unwrap_or_else(|_| fail("set CHROME to a (headless) Chromium binary"));
        let pipes = std::io::pipe().and_then(|a| Ok((a, std::io::pipe()?)));
        let ((reads, to), (from, writes)) = pipes.unwrap_or_else(|e| fail(&format!("a pipe: {e}")));
        let (r, w) = (reads.as_raw_fd(), writes.as_raw_fd());
        let mut cmd = Command::new(bin);
        cmd.args(["--headless", "--no-sandbox", "--remote-debugging-pipe", &format!("--window-size={width},{height}"), "about:blank"])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // (Its ends as fds 3 and 4: moved out of the way first, as they may be 3 or 4 already.)
        unsafe {
            cmd.pre_exec(move || {
                let (r, w) = (fcntl(r, 0, 10), fcntl(w, 0, 10));
                if r < 0 || w < 0 || dup2(r, 3) < 0 || dup2(w, 4) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = cmd.spawn().unwrap_or_else(|e| fail(&format!("$CHROME: {e}")));
        drop((reads, writes));
        let (tx, replies) = channel();
        let exceptions = Arc::new(AtomicUsize::new(0));
        let seen = exceptions.clone();
        std::thread::spawn(move || {
            let mut from = BufReader::new(from);
            let mut buf = Vec::new();
            while from.read_until(0, &mut buf).is_ok_and(|n| n > 0) {
                let m = Json::parse(String::from_utf8_lossy(buf.strip_suffix(&[0]).unwrap_or(&buf)).as_ref());
                buf.clear();
                let Some(m) = m else { continue };
                let p = m.get("params");
                match m.get("method").and_then(Json::str) {
                    Some("Runtime.consoleAPICalled") => {
                        let args = p.and_then(|p| p.get("args")).map_or(&[][..], Json::list);
                        let said: Vec<String> = args.iter().map(|a| a.get("value").or(a.get("description")).map_or(String::new(), Json::text)).collect();
                        println!("{}", said.join(" "));
                    }
                    Some("Runtime.exceptionThrown") => {
                        seen.fetch_add(1, Ordering::SeqCst);
                        let d = p.and_then(|p| p.get("exceptionDetails"));
                        let what = d.and_then(|d| d.get("exception")).and_then(|e| e.get("description")).or(d.and_then(|d| d.get("text")));
                        println!("exception: {}", what.map_or(String::new(), Json::text));
                    }
                    Some(_) => {}
                    None => {
                        let _ = tx.send(m);
                    }
                }
            }
        });
        let mut c = Chrome { child, to, replies, id: 0, session: None, exceptions };
        // (A page of its own, attached: the commands after go to it.)
        let target = c.call("Target.createTarget", r#"{"url":"about:blank"}"#).ok().and_then(|r| r.get("targetId").and_then(Json::str).map(String::from));
        let target = target.unwrap_or_else(|| fail("Chromium: no page"));
        let session = c.call("Target.attachToTarget", &format!(r#"{{"targetId":{},"flatten":true}}"#, quote(&target))).ok()
            .and_then(|r| r.get("sessionId").and_then(Json::str).map(String::from));
        c.session = Some(session.unwrap_or_else(|| fail("Chromium: the page not attached")));
        for (method, params) in [("Runtime.enable", "{}".to_string()), ("Page.enable", "{}".to_string()),
                                 ("Emulation.setDeviceMetricsOverride", format!(r#"{{"width":{width},"height":{height},"deviceScaleFactor":1,"mobile":false}}"#))] {
            c.call(method, &params).unwrap_or_else(|e| fail(&e));
        }
        c
    }

    /// Call `method` with `params` (a JSON object): its result, or its error.
    pub fn call(&mut self, method: &str, params: &str) -> Result<Json, String> {
        self.id += 1;
        let session = self.session.as_ref().map_or(String::new(), |s| format!(r#","sessionId":{}"#, quote(s)));
        let m = format!(r#"{{"id":{},"method":"{method}","params":{params}{session}}}"#, self.id);
        self.to.write_all(m.as_bytes()).and_then(|_| self.to.write_all(&[0])).map_err(|e| format!("{method}: {e}"))?;
        loop {
            // (Long enough for the kernel's tests.)
            let r = self.replies.recv_timeout(Duration::from_secs(900)).map_err(|_| format!("{method}: no reply"))?;
            if r.get("id").and_then(Json::num) != Some(self.id as f64) {
                continue;
            }
            if let Some(e) = r.get("error") {
                return Err(format!("{method}: {}", e.get("message").map_or(String::new(), Json::text)));
            }
            return Ok(r.get("result").cloned().unwrap_or(Json::Null));
        }
    }

    /// Script `js` evaluated in the page (a promise, awaited): its value,
    /// or what it threw.
    pub fn eval(&mut self, js: &str) -> Result<Json, String> {
        let r = self.call("Runtime.evaluate", &format!(r#"{{"expression":{},"awaitPromise":true,"returnByValue":true}}"#, quote(js)))?;
        if let Some(d) = r.get("exceptionDetails") {
            // (An error, its description; a value thrown (a test's failure: a string), it.)
            let e = d.get("exception");
            let what = e.and_then(|e| e.get("description").or(e.get("value"))).or(d.get("text"));
            return Err(what.map_or("an exception".into(), Json::text));
        }
        Ok(r.get("result").and_then(|v| v.get("value")).cloned().unwrap_or(Json::Null))
    }

    /// The page as it looks, as a PNG in `out`.
    pub fn screenshot(&mut self, out: &Path) {
        let r = self.call("Page.captureScreenshot", r#"{"format":"png"}"#).unwrap_or_else(|e| fail(&e));
        let png = r.get("data").and_then(Json::str).and_then(base64).unwrap_or_else(|| fail("a screenshot, not base64"));
        std::fs::write(out, png).unwrap_or_else(|e| fail(&format!("{}: {e}", out.display())));
    }

    pub fn exceptions(&self) -> usize {
        self.exceptions.load(Ordering::SeqCst)
    }
}

impl Drop for Chrome {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The tests (crates/ksee-page's uitest.rs), each with the page it opens
/// (its URL's hash) and the screen it has: the kernel's (the page not
/// mounted, the Machine theirs), then the page's: the read example, the
/// write example with its program changed in the URL, the pipe example,
/// the read example on a phone's screen.
pub const TESTS: &[(&str, &str, (u32, u32))] = &[("kernel", "#selftest", SCREEN), ("read", "#ex=read", SCREEN), ("back", "", SCREEN),
                                                  ("pipe", "#ex=pipe", SCREEN), ("phone", "#ex=read", (390, 844))];
const SCREEN: (u32, u32) = (1500, 900);
const BACK: &str = r#"int fd = openat(AT_FDCWD, "/note.txt", O_WRONLY | O_APPEND); write(fd, "changed;\n", 9)"#;

/// What to look at: the site at `hash` (or `url`), a screen `size`, after
/// `wait` ms a script (`eval`), then a screenshot in `out`.
pub struct Shot {
    pub site: PathBuf,
    pub url: Option<String>,
    pub hash: String,
    pub size: (u32, u32),
    pub wait: u64,
    pub eval: Option<String>,
    pub out: Option<PathBuf>,
}

impl Shot {
    /// Test `name`'s, on the site built with the tests (target/test/site),
    /// its screenshot in `out`.
    pub fn test(name: &str, out: Option<PathBuf>) -> Shot {
        let Some(&(_, hash, size)) = TESTS.iter().find(|t| t.0 == name) else {
            fail(&format!("no test {name}: {}", TESTS.iter().map(|t| t.0).collect::<Vec<_>>().join(", ")))
        };
        let hash = if name == "back" { format!("#ex=write&p={}", encode(BACK)) } else { hash.into() };
        // (Once the page has them: mount gives them, the kernel booted.)
        let eval = format!(r#"new Promise((ok) => {{ const t = setInterval(() => window.ksee_test && (clearInterval(t), ok(window.ksee_test({}))), 100); }})"#, quote(name));
        Shot { site: "target/test/site".into(), url: None, hash, size, wait: 0, eval: Some(eval), out }
    }

    /// Look: whether the script ran (if one) and the page threw nothing.
    pub fn run(&self) -> bool {
        let url = self.url.clone().unwrap_or_else(|| {
            // (Served here, on a port of its own.)
            let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| fail(&format!("serve: {e}")));
            let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
            let site = self.site.clone();
            std::thread::spawn(move || crate::serve::serve_on(listener, &site));
            format!("http://127.0.0.1:{port}/{}", self.hash)
        });
        let mut c = Chrome::start(self.size.0, self.size.1);
        c.call("Page.navigate", &format!(r#"{{"url":{}}}"#, quote(&url))).unwrap_or_else(|e| fail(&e));
        std::thread::sleep(Duration::from_millis(self.wait));
        let ran = self.eval.as_ref().is_none_or(|js| match c.eval(js) {
            Ok(v) => {
                println!("eval: {}", v.text());
                true
            }
            Err(e) => {
                println!("eval failed: {e}");
                false
            }
        });
        if let Some(out) = &self.out {
            c.screenshot(out);
            println!("saved {}", out.display());
        }
        let ok = ran && c.exceptions() == 0;
        println!("{}", if ok { "PASS".to_string() } else { format!("FAIL ({} exceptions)", c.exceptions()) });
        ok
    }
}

/// `s` as a URL's component (encodeURIComponent).
fn encode(s: &str) -> String {
    s.bytes().map(|b| match b {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')' => (b as char).to_string(),
        _ => format!("%{b:02X}"),
    }).collect()
}

/// `s` as a JSON string.
fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Base64 (standard, padded) decoded.
fn base64(s: &str) -> Option<Vec<u8>> {
    let digit = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    let mut out = Vec::new();
    for quad in s.trim_end_matches('=').as_bytes().chunks(4) {
        let d: Vec<u8> = quad.iter().map(|&c| digit(c)).collect::<Option<_>>()?;
        let n = d.iter().enumerate().fold(0u32, |n, (k, &x)| n | (x as u32) << (18 - 6 * k));
        out.extend_from_slice(&n.to_be_bytes()[1..d.len()]);
    }
    Some(out)
}

/// A JSON value (what CDP sends).
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    List(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    /// `s` read as JSON, if it is.
    pub fn parse(s: &str) -> Option<Json> {
        let mut rest = s;
        let v = value(&mut rest)?;
        rest.trim().is_empty().then_some(v)
    }

    /// An object's member `k`.
    pub fn get(&self, k: &str) -> Option<&Json> {
        match self {
            Json::Obj(m) => m.iter().find(|(n, _)| n == k).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn num(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn list(&self) -> &[Json] {
        match self {
            Json::List(l) => l,
            _ => &[],
        }
    }

    /// As said to a person: a string as it is, anything else as JSON.
    pub fn text(&self) -> String {
        match self {
            Json::Null => "null".into(),
            Json::Bool(b) => b.to_string(),
            Json::Num(n) => n.to_string(),
            Json::Str(s) => s.clone(),
            Json::List(l) => format!("[{}]", l.iter().map(Json::text).collect::<Vec<_>>().join(", ")),
            Json::Obj(m) => format!("{{{}}}", m.iter().map(|(k, v)| format!("{k}: {}", v.text())).collect::<Vec<_>>().join(", ")),
        }
    }
}

/// The value at the start of `s`, `s` moved past it.
fn value(s: &mut &str) -> Option<Json> {
    *s = s.trim_start();
    let (open, close) = match s.chars().next()? {
        '"' => return string(s).map(Json::Str),
        '{' => ('{', '}'),
        '[' => ('[', ']'),
        _ => {
            for (word, v) in [("true", Json::Bool(true)), ("false", Json::Bool(false)), ("null", Json::Null)] {
                if let Some(r) = s.strip_prefix(word) {
                    *s = r;
                    return Some(v);
                }
            }
            let end = s.find(|c: char| !(c.is_ascii_digit() || "+-.eE".contains(c))).unwrap_or(s.len());
            let n = s[..end].parse().ok()?;
            *s = &s[end..];
            return Some(Json::Num(n));
        }
    };
    *s = &s[1..];
    let (mut list, mut obj) = (Vec::new(), Vec::new());
    loop {
        *s = s.trim_start();
        if let Some(r) = s.strip_prefix(close) {
            *s = r;
            return Some(if open == '{' { Json::Obj(obj) } else { Json::List(list) });
        }
        if open == '{' {
            let k = string(s)?;
            *s = s.trim_start().strip_prefix(':')?;
            obj.push((k, value(s)?));
        } else {
            list.push(value(s)?);
        }
        *s = s.trim_start();
        if let Some(r) = s.strip_prefix(',') {
            *s = r;
        }
    }
}

/// The string at the start of `s` (quoted), `s` moved past it.
fn string(s: &mut &str) -> Option<String> {
    let mut it = s.strip_prefix('"')?.char_indices();
    let body = &s[1..];
    let mut out = String::new();
    let hex = |it: &mut std::str::CharIndices| -> Option<u32> { (0..4).try_fold(0, |n, _| Some(n * 16 + it.next()?.1.to_digit(16)?)) };
    loop {
        let (i, c) = it.next()?;
        match c {
            '"' => {
                *s = &body[i + 1..];
                return Some(out);
            }
            '\\' => out.push(match it.next()?.1 {
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                'b' => '\u{8}',
                'f' => '\u{c}',
                'u' => {
                    let mut u = hex(&mut it)?;
                    // (A surrogate pair: its second half next.)
                    if (0xd800..0xdc00).contains(&u) && it.next()?.1 == '\\' && it.next()?.1 == 'u' {
                        u = 0x10000 + ((u - 0xd800) << 10) + (hex(&mut it)? - 0xdc00);
                    }
                    char::from_u32(u).unwrap_or('\u{fffd}')
                }
                c => c,
            }),
            c => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_as_cdp_sends_it() {
        let m = Json::parse(r#"{"id":3,"result":{"result":{"type":"string","value":"a \"b\"\né 😀"}},"list":[1,true,null]}"#).unwrap();
        assert_eq!(m.get("id").and_then(Json::num), Some(3.0));
        assert_eq!(m.get("result").and_then(|r| r.get("result")).and_then(|r| r.get("value")).and_then(Json::str), Some("a \"b\"\né 😀"));
        assert_eq!(m.get("list").map(Json::list), Some(&[Json::Num(1.0), Json::Bool(true), Json::Null][..]));
        assert_eq!(Json::parse(&quote("x\"\\\n\u{1}")), Some(Json::Str("x\"\\\n\u{1}".into())));
        assert_eq!(Json::parse("{\"a\":1} x"), None);
    }

    #[test]
    fn base64_and_a_urls_component() {
        assert_eq!(base64("aGVsbG8="), Some(b"hello".to_vec()));
        assert_eq!(base64("aGk="), Some(b"hi".to_vec()));
        assert_eq!(base64("YWJj"), Some(b"abc".to_vec()));
        assert_eq!(encode(r#"write(fd, "a;\n", 3)"#), "write(fd%2C%20%22a%3B%5Cn%22%2C%203)");
    }
}
