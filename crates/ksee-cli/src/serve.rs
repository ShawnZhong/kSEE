//! `ksee serve`: a static file server for the site. `ksee test` serves the
//! test site on it too, and the page POSTs each test's outcome (`ok` or
//! `fail`, then its log) to /result, for the waiting test thread.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::Sender;

pub fn serve(root: &Path, port: u16) -> std::io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    eprintln!("serving {} at http://127.0.0.1:{}/", root.display(), listener.local_addr()?.port());
    serve_on(listener, root, None);
    Ok(())
}

/// A test's outcome, as its page POSTed it to /result: whether it passed,
/// and what it said.
pub struct Outcome {
    pub passed: bool,
    pub log: String,
}

/// Serve `root` on `listener`, for ever; each POST to /result handed to
/// `outcomes`, if one is given.
pub fn serve_on(listener: TcpListener, root: &Path, outcomes: Option<Sender<Outcome>>) {
    for stream in listener.incoming().flatten() {
        let (root, outcomes) = (root.to_path_buf(), outcomes.clone());
        std::thread::spawn(move || {
            let _ = handle(&root, stream, &outcomes);
        });
    }
}

fn handle(root: &Path, mut stream: TcpStream, outcomes: &Option<Sender<Outcome>>) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request = String::new();
    reader.read_line(&mut request)?;
    let mut length = 0;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? <= 2 {
            break;
        }
        if let Some(n) = line.strip_prefix("Content-Length:").or(line.strip_prefix("content-length:")) {
            length = n.trim().parse().unwrap_or(0);
        }
    }
    let mut parts = request.split_whitespace();
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
    let path = path.split(['?', '#']).next().unwrap_or("/");
    if method == "POST" && path == "/result" {
        let mut body = vec![0; length];
        reader.read_exact(&mut body)?;
        let body = String::from_utf8_lossy(&body);
        let (first, log) = body.split_once('\n').unwrap_or((body.as_ref(), ""));
        if let Some(outcomes) = outcomes {
            let _ = outcomes.send(Outcome { passed: first.trim() == "ok", log: log.to_string() });
        }
        return reply(&mut stream, "200 OK", "text/plain", b"ok\n");
    }
    let (status, body, kind) = match resolve(root, path).and_then(|p| Some((std::fs::read(&p).ok()?, p))) {
        Some((body, p)) => ("200 OK", body, mime(&p)),
        None => ("404 Not Found", b"not found\n".to_vec(), "text/plain"),
    };
    reply(&mut stream, status, kind, &body)
}

fn reply(stream: &mut TcpStream, status: &str, kind: &str, body: &[u8]) -> std::io::Result<()> {
    write!(stream, "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\n\
                    Cache-Control: no-store\r\nConnection: close\r\n\r\n", body.len())?;
    stream.write_all(body)
}

/// The file for a URL path, if it is inside `root` (index.html for dirs).
/// (No decoding: a % in the path stays a %, so no URL smuggles a `..` past
/// the components below.)
fn resolve(root: &Path, url: &str) -> Option<PathBuf> {
    let mut p = root.to_path_buf();
    for c in Path::new(url.trim_start_matches('/')).components() {
        match c {
            Component::Normal(s) => p.push(s),
            Component::CurDir => {}
            _ => return None,
        }
    }
    if p.is_dir() {
        p.push("index.html");
    }
    p.is_file().then_some(p)
}

fn mime(p: &Path) -> &'static str {
    match p.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript",
        Some("json") => "application/json",
        Some("wasm") => "application/wasm",
        Some("css") => "text/css",
        Some("gz") => "application/gzip",
        _ => match p.file_name().and_then(|n| n.to_str()) {
            Some("vmlinux") => "application/wasm",
            _ => "application/octet-stream",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A scratch dir, removed on drop (even on panic).
    struct Scratch(PathBuf);
    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir = std::env::temp_dir().join(format!("ksee-{name}-{}-{}", std::process::id(),
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |t| t.subsec_nanos())));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn mime_kinds() {
        let mime_of = |name: &str| mime(Path::new(name));
        assert_eq!(mime_of("index.html"), "text/html; charset=utf-8");
        assert_eq!(mime_of("ksee.js"), "text/javascript");
        assert_eq!(mime_of("examples.json"), "application/json");
        assert_eq!(mime_of("ksee_page_bg.wasm"), "application/wasm");
        assert_eq!(mime_of("vmlinux"), "application/wasm");
        assert_eq!(mime_of("vmlinux.gz"), "application/gzip");
        assert_eq!(mime_of("note.txt"), "application/octet-stream");
    }

    #[test]
    fn resolve_stays_inside_root() {
        let root = Scratch::new("serve");
        std::fs::create_dir_all(root.0.join("sub")).unwrap();
        std::fs::write(root.0.join("index.html"), "hi").unwrap();
        std::fs::write(root.0.join("sub").join("examples.json"), "{}").unwrap();
        assert_eq!(resolve(&root.0, "/"), Some(root.0.join("index.html")));
        assert_eq!(resolve(&root.0, "/sub/examples.json"), Some(root.0.join("sub").join("examples.json")));
        assert_eq!(resolve(&root.0, "/../secret"), None);
        assert_eq!(resolve(&root.0, "/missing"), None);
        assert_eq!(resolve(&root.0, "/%2e%2e/secret"), None);
    }

    #[test]
    fn result_posts_reach_the_waiting_thread() {
        let root = Scratch::new("result");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = std::sync::mpsc::channel();
        let site = root.0.clone();
        std::thread::spawn(move || serve_on(listener, &site, Some(tx)));
        for (body, passed) in [("ok\nall good\n", true), ("fail\nwhat broke\n", false)] {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(stream, "POST /result HTTP/1.1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            let outcome = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
            assert_eq!((outcome.passed, outcome.log), (passed, body.split_once('\n').unwrap().1.into()));
        }
    }
}
