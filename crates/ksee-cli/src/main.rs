//! The ksee command (./ksee.sh runs it, in the kSEE tree):
//!
//!   ksee                     build, then serve the page (as serve)
//!   ksee build               build the kernel, runtime and page (the
//!                            Makefile's `make site`)
//!   ksee test [--built] [--ui | NAME ...]
//!                            build, then run named Chromium tests; --built
//!                            uses an existing production build
//!   ksee deploy              build, then push the page to origin's gh-pages
//!   ksee patch               write linux.patch from linux/ (the port, as it
//!                            differs from the release)
//!   ksee serve [--port=N]    serve target/site (the page, as built) for
//!                            a browser
//!
//! and the build's steps that are code (the Makefile runs them):
//!
//!   ksee pack DEBUG LINUX TABLES SOURCES
//!                            the kernel's tables, of its description
//!                            DEBUG and the tree LINUX, and the sources
//!                            they name (pack/)
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
                     | pack DEBUG LINUX TABLES SOURCES | lessons | bindgen WASM OUT]";

#[derive(Debug, Eq, PartialEq)]
enum Command {
    Build,
    Test { built: bool, names: Vec<String> },
    Deploy,
    Patch,
    /// (`build`: build first, as plain `ksee` does.)
    Serve { build: bool, port: u16 },
    Pack { debug: String, linux: String, tables: String, sources: String },
    Lessons,
    Bindgen { wasm: String, out: String },
}

fn parse_args(args: &[&str]) -> Result<Command, String> {
    let Some((command, options)) = args.split_first() else {
        return Ok(Command::Serve { build: true, port: 8000 });
    };
    match *command {
        "pack" => match options {
            [debug, linux, tables, sources] => Ok(Command::Pack { debug: debug.to_string(), linux: linux.to_string(), tables: tables.to_string(), sources: sources.to_string() }),
            _ => Err("pack takes DEBUG LINUX TABLES SOURCES".into()),
        },
        "bindgen" => match options {
            [wasm, out] => Ok(Command::Bindgen { wasm: wasm.to_string(), out: out.to_string() }),
            _ => Err("bindgen takes WASM OUT".into()),
        },
        "build" | "deploy" | "patch" | "lessons" if !options.is_empty() => {
            Err(format!("{command} takes no options or arguments"))
        }
        "build" => Ok(Command::Build),
        "deploy" => Ok(Command::Deploy),
        "patch" => Ok(Command::Patch),
        "lessons" => Ok(Command::Lessons),
        "test" => {
            let (built, selection) = match options.first() {
                Some(&"--built") => (true, &options[1..]),
                _ => (false, options),
            };
            let names: Vec<String> = match selection {
                [] => vec!["kernel".into(), "lessons".into()],
                ["--ui"] => chrome::TESTS.iter().map(|test| test.0.into()).collect(),
                names if names.iter().all(|name| chrome::TESTS.iter().any(|test| test.0 == *name)) =>
                    names.iter().map(|name| (*name).into()).collect(),
                _ => return Err(format!("test names are: {}", chrome::TESTS.iter().map(|test| test.0).collect::<Vec<_>>().join(", "))),
            };
            Ok(Command::Test { built, names })
        }
        "serve" => match options {
            [] => Ok(Command::Serve { build: false, port: 8000 }),
            [option] => parse_port(option).map(|port| Command::Serve { build: false, port }),
            _ => Err("serve accepts only one optional --port=N".into()),
        },
        _ if command.starts_with('-') => Err(format!("unknown option: {command}")),
        _ => Err(format!("unknown command: {command}")),
    }
}

fn parse_port(option: &str) -> Result<u16, String> {
    let value = option.strip_prefix("--port=")
        .ok_or_else(|| "expected --port=N".to_string())?;
    value.parse::<u16>().map_err(|_| format!("invalid port: {value}"))
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
    let command = match parse_args(&args) {
        Ok(command) => command,
        Err(error) => {
            eprintln!("ksee: {error}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match command {
        Command::Build => build::build(),
        Command::Test { built, names } => build::test(&names, built),
        Command::Deploy => deploy::deploy(),
        Command::Patch => build::patch(),
        Command::Serve { build: first, port } => {
            if first {
                build::build();
            }
            or_fail(serve::serve(Path::new(build::SITE), port), "serve");
        }
        Command::Pack { debug, linux, tables, sources } => pack::kernel(Path::new(&debug), Path::new(&linux), Path::new(&tables), Path::new(&sources)),
        Command::Lessons => lessons::write(),
        Command::Bindgen { wasm, out } => build::bindgen(&wasm, &out),
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Command {
        parse_args(args).unwrap()
    }

    #[test]
    fn parses_commands_and_their_options() {
        assert_eq!(parse(&[]), Command::Serve { build: true, port: 8000 });
        assert_eq!(parse(&["build"]), Command::Build);
        assert_eq!(parse(&["test"]), Command::Test { built: false, names: vec!["kernel".into(), "lessons".into()] });
        assert_eq!(parse(&["test", "read"]), Command::Test { built: false, names: vec!["read".into()] });
        assert_eq!(parse(&["test", "--built", "read", "responsive"]), Command::Test { built: true, names: vec!["read".into(), "responsive".into()] });
        assert_eq!(parse(&["test", "--ui"]), Command::Test { built: false, names: chrome::TESTS.iter().map(|test| test.0.into()).collect() });
        assert_eq!(parse(&["test", "--built", "--ui"]), Command::Test { built: true, names: chrome::TESTS.iter().map(|test| test.0.into()).collect() });
        assert_eq!(parse(&["deploy"]), Command::Deploy);
        assert_eq!(parse(&["patch"]), Command::Patch);
        assert_eq!(parse(&["serve"]), Command::Serve { build: false, port: 8000 });
        assert_eq!(parse(&["serve", "--port=9000"]), Command::Serve { build: false, port: 9000 });
        assert_eq!(parse(&["serve", "--port=0"]), Command::Serve { build: false, port: 0 });
        assert_eq!(parse(&["pack", "d", "l", "t", "s"]), Command::Pack { debug: "d".into(), linux: "l".into(), tables: "t".into(), sources: "s".into() });
        assert_eq!(parse(&["lessons"]), Command::Lessons);
        assert_eq!(parse(&["bindgen", "w", "o"]), Command::Bindgen { wasm: "w".into(), out: "o".into() });
    }

    #[test]
    fn rejects_invalid_ports() {
        for port in ["", "no", "65536"] {
            assert!(parse_args(&["serve", &format!("--port={port}")]).is_err());
        }
        assert!(parse_args(&["serve", "--port", "9000"]).is_err());
        assert!(parse_args(&["serve", "--port=8000", "--port=9000"]).is_err());
    }

    #[test]
    fn rejects_unknown_misplaced_and_extra_arguments() {
        for args in [
            &["--unknown"][..],
            &["--port=9000"][..],
            &["unknown"][..],
            &["build", "--ui"][..],
            &["deploy", "extra"][..],
            &["patch", "--unknown"][..],
            &["pack", "d"][..],
            &["lessons", "x"][..],
            &["bindgen", "w"][..],
            &["test", "--port=9000"][..],
            &["test", "--ui", "--ui"][..],
            &["serve", "--ui"][..],
        ] {
            assert!(parse_args(args).is_err(), "accepted {args:?}");
        }
    }
}
