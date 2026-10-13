//! The ksee command (./ksee.sh runs it, in the kSEE tree):
//!
//!   ksee                     build, then serve the page (as serve)
//!   ksee build               build the kernel, runtime and page (the
//!                            Makefile's `make site`)
//!   ksee test [--built] [--ui | NAME ...]
//!                            native tests and build, then the named browser
//!                            tests; --built: earlier steps (CI's) did both
//!   ksee deploy              build, then push the page to origin's gh-pages
//!   ksee patch               write linux.patch from linux/ (the port, as it
//!                            differs from the release)
//!   ksee serve [--port=N]    serve build/site (the page, as built) for
//!                            a browser
//!
//! and the build's steps that are code (the Makefile runs them):
//!
//!   ksee tables DEBUG HEADERS OUT
//!                            the kernel's tables, of its description
//!                            DEBUG and its UAPI HEADERS (pack/)
//!   ksee sources DEBUG LINUX OUT
//!                            the sources DEBUG names, of the tree LINUX
//!   ksee lessons             lessons.json and the lessons' pages (lessons.rs)
//!   ksee bindgen WASM OUT    wasm-bindgen's glue for WASM, in OUT
mod build;
mod chrome;
mod deploy;
mod lessons;
mod pack;
mod serve;

use std::fmt::Display;
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "usage: ksee [build | test [--built] [--ui | NAME ...] | deploy | serve [--port=N] | patch \
                     | tables DEBUG HEADERS OUT | sources DEBUG LINUX OUT | lessons | bindgen WASM OUT]";

/// The tests `selection` names: none, the kernel's and the lessons'; --ui,
/// all; else those named, if each is one.
fn test_names(selection: &[&str]) -> Option<Vec<String>> {
    let all = || chrome::TESTS.iter().map(|test| test.0);
    match selection {
        [] => Some(vec!["kernel".into(), "lessons".into()]),
        ["--ui"] => Some(all().map(String::from).collect()),
        names if names.iter().all(|n| all().any(|t| t == *n)) => Some(names.iter().map(|n| n.to_string()).collect()),
        _ => None,
    }
}

fn serve(port: u16) {
    or_fail(serve::serve(Path::new(build::SITE), port), "serve");
}

fn read(p: &Path) -> Vec<u8> {
    or_fail(std::fs::read(p), p.display())
}

/// `r`'s value, or a failure: `what` and its error.
fn or_fail<T, E: Display>(r: Result<T, E>, what: impl Display) -> T {
    r.unwrap_or_else(|e| fail(&format!("{what}: {e}")))
}

fn fail(msg: &str) -> ! {
    eprintln!("ksee: {msg}");
    std::process::exit(1)
}

/// A test's scratch dir, removed on drop (even on panic).
#[cfg(test)]
struct Scratch(std::path::PathBuf);

#[cfg(test)]
impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("ksee-{name}-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |t| t.subsec_nanos())));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

#[cfg(test)]
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let p = Path::new;
    match args[..] {
        [] => {
            build::build();
            serve(8000);
        }
        ["build"] => build::build(),
        ["test", ref selection @ ..] => {
            let (built, selection) = match selection {
                ["--built", rest @ ..] => (true, rest),
                rest => (false, rest),
            };
            let Some(names) = test_names(selection) else { return usage() };
            build::test(&names, built);
        }
        ["deploy"] => deploy::deploy(),
        ["patch"] => build::patch(),
        ["serve"] => serve(8000),
        ["serve", port] => match port.strip_prefix("--port=").and_then(|v| v.parse().ok()) {
            Some(port) => serve(port),
            None => return usage(),
        },
        ["tables", debug, headers, out] => pack::tables(p(debug), p(headers), p(out)),
        ["sources", debug, linux, out] => pack::sources(p(debug), p(linux), p(out)),
        ["lessons"] => lessons::write(),
        ["bindgen", wasm, out] => build::bindgen(wasm, out),
        _ => return usage(),
    }
    ExitCode::SUCCESS
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}\n(test names: {})", chrome::TESTS.iter().map(|t| t.0).collect::<Vec<_>>().join(", "));
    ExitCode::from(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_names_are_the_tests() {
        assert_eq!(test_names(&[]), Some(vec!["kernel".into(), "lessons".into()]));
        assert_eq!(test_names(&["read"]), Some(vec!["read".into()]));
        assert_eq!(test_names(&["--ui"]).map(|n| n.len()), Some(chrome::TESTS.len()));
        assert_eq!(test_names(&["unknown"]), None);
        assert_eq!(test_names(&["--ui", "--ui"]), None);
    }
}
