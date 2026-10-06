//! `ksee serve`: a static file server for the site (`ksee test` serves the
//! site on it too).

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};

pub fn serve(root: &Path, port: u16) -> std::io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    eprintln!("serving {} at http://127.0.0.1:{}/", root.display(), listener.local_addr()?.port());
    serve_on(listener, root);
    Ok(())
}

/// Serve `root` on `listener`, for ever.
pub fn serve_on(listener: TcpListener, root: &Path) {
    for stream in listener.incoming().flatten() {
        let root = root.to_path_buf();
        std::thread::spawn(move || {
            let _ = handle(&root, stream);
        });
    }
}

fn handle(root: &Path, mut stream: TcpStream) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request = String::new();
    reader.read_line(&mut request)?;
    // (The headers, read past: none matters.)
    let mut line = String::new();
    while reader.read_line(&mut line)? > 2 {
        line.clear();
    }
    let path = request.split_whitespace().nth(1).unwrap_or("/");
    let path = path.split(['?', '#']).next().unwrap_or("/");
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

/// The types the browser checks: the page, its module scripts, and the
/// runtime's module (instantiateStreaming). (The page reads the rest as
/// bytes or text, whatever the type.)
fn mime(p: &Path) -> &'static str {
    match p.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Scratch;

    #[test]
    fn mime_kinds() {
        let mime_of = |name: &str| mime(Path::new(name));
        assert_eq!(mime_of("index.html"), "text/html; charset=utf-8");
        assert_eq!(mime_of("x.js"), "text/javascript");
        assert_eq!(mime_of("ksee_page_bg.wasm"), "application/wasm");
        assert_eq!(mime_of("vmlinux.gz"), "application/octet-stream");
    }

    #[test]
    fn resolve_stays_inside_root() {
        let root = Scratch::new("serve");
        std::fs::create_dir_all(root.0.join("sub")).unwrap();
        std::fs::write(root.0.join("index.html"), "hi").unwrap();
        std::fs::write(root.0.join("sub").join("lessons.json"), "{}").unwrap();
        assert_eq!(resolve(&root.0, "/"), Some(root.0.join("index.html")));
        assert_eq!(resolve(&root.0, "/sub/lessons.json"), Some(root.0.join("sub").join("lessons.json")));
        assert_eq!(resolve(&root.0, "/../secret"), None);
        assert_eq!(resolve(&root.0, "/missing"), None);
        assert_eq!(resolve(&root.0, "/%2e%2e/secret"), None);
    }
}
