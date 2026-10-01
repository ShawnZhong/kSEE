//! `ksee serve`: a static file server for the site.

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
    // Skip the headers.
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
    write!(stream, "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\n\
                    Cache-Control: no-store\r\nConnection: close\r\n\r\n", body.len())?;
    stream.write_all(&body)
}

/// The file for a URL path, if it is inside `root` (index.html for dirs).
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
        Some("wasm") => "application/wasm",
        Some("css") => "text/css",
        Some("gz") => "application/gzip",
        _ => match p.file_name().and_then(|n| n.to_str()) {
            Some("vmlinux") => "application/wasm",
            _ => "application/octet-stream",
        },
    }
}
