//! The ksee command (./ksee.sh runs it, in the kSEE tree):
//!
//!   ksee [--port=N]          build, then serve the page
//!   ksee build               build the kernel, runtime and page (build.rs)
//!   ksee test [--ui]         build, then run the tests (in Chromium,
//!                            $CHROME; --ui: the page's too)
//!   ksee deploy              build, then push the page to origin's gh-pages
//!   ksee patch               write linux.patch from linux/ (the port, as it
//!                            differs from the release)
//!   ksee serve [--port=N]    serve target/site (the page, as built) for
//!                            a browser
mod build;
mod chrome;
mod deploy;
mod pack;
mod serve;

use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "usage: ksee [--port=N | build | test [--ui] | deploy | serve [--port=N] | patch]";

#[derive(Debug, Eq, PartialEq)]
enum Command {
    BuildAndServe { port: u16 },
    Build,
    Test { ui: bool },
    Deploy,
    Patch,
    Serve { port: u16 },
}

fn parse_args(args: &[&str]) -> Result<Command, String> {
    let Some((command, options)) = args.split_first() else {
        return Ok(Command::BuildAndServe { port: 8000 });
    };
    if let [option] = args {
        if option.starts_with("--port=") {
            return parse_port(option).map(|port| Command::BuildAndServe { port });
        }
    }
    match *command {
        "build" | "deploy" | "patch" if !options.is_empty() => {
            Err(format!("{command} takes no options or arguments"))
        }
        "build" => Ok(Command::Build),
        "deploy" => Ok(Command::Deploy),
        "patch" => Ok(Command::Patch),
        "test" => match options {
            [] => Ok(Command::Test { ui: false }),
            ["--ui"] => Ok(Command::Test { ui: true }),
            _ => Err("test accepts only one optional --ui".into()),
        },
        "serve" => match options {
            [] => Ok(Command::Serve { port: 8000 }),
            [option] => parse_port(option).map(|port| Command::Serve { port }),
            _ => Err("serve accepts only one optional --port=N".into()),
        },
        _ if command.starts_with('-') => Err(format!("unknown option: {command}")),
        _ => Err(format!("unknown command: {command}")),
    }
}

fn parse_port(option: &str) -> Result<u16, String> {
    let value = option.strip_prefix("--port=")
        .ok_or_else(|| "expected --port=N".to_string())?;
    let port = value.parse::<u16>().map_err(|_| format!("invalid port: {value}"))?;
    if port == 0 {
        return Err("invalid port: 0".into());
    }
    Ok(port)
}

fn read(p: &Path) -> Vec<u8> {
    std::fs::read(p).unwrap_or_else(|e| fail(&format!("{}: {e}", p.display())))
}

fn fail(msg: &str) -> ! {
    eprintln!("ksee: {msg}");
    std::process::exit(1)
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
        Command::BuildAndServe { port } => {
            build::build();
            serve::serve(Path::new(build::SITE), port).unwrap_or_else(|e| fail(&format!("serve: {e}")));
        }
        Command::Build => build::build(),
        Command::Test { ui } => build::test(ui),
        Command::Deploy => deploy::deploy(),
        Command::Patch => build::patch(),
        Command::Serve { port } => {
            serve::serve(Path::new(build::SITE), port).unwrap_or_else(|e| fail(&format!("serve: {e}")));
        }
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
        assert_eq!(parse(&[]), Command::BuildAndServe { port: 8000 });
        assert_eq!(parse(&["--port=9000"]), Command::BuildAndServe { port: 9000 });
        assert_eq!(parse(&["build"]), Command::Build);
        assert_eq!(parse(&["test"]), Command::Test { ui: false });
        assert_eq!(parse(&["test", "--ui"]), Command::Test { ui: true });
        assert_eq!(parse(&["deploy"]), Command::Deploy);
        assert_eq!(parse(&["patch"]), Command::Patch);
        assert_eq!(parse(&["serve"]), Command::Serve { port: 8000 });
        assert_eq!(parse(&["serve", "--port=9000"]), Command::Serve { port: 9000 });
    }

    #[test]
    fn rejects_invalid_ports() {
        for port in ["", "no", "0", "65536"] {
            assert!(parse_args(&["serve", &format!("--port={port}")]).is_err());
        }
        assert!(parse_args(&["serve", "--port", "9000"]).is_err());
        assert!(parse_args(&["serve", "--port=8000", "--port=9000"]).is_err());
    }

    #[test]
    fn rejects_unknown_misplaced_and_extra_arguments() {
        for args in [
            &["--unknown"][..],
            &["unknown"][..],
            &["build", "--ui"][..],
            &["deploy", "extra"][..],
            &["patch", "--unknown"][..],
            &["test", "--port=9000"][..],
            &["test", "--ui", "--ui"][..],
            &["serve", "--ui"][..],
        ] {
            assert!(parse_args(args).is_err(), "accepted {args:?}");
        }
    }
}
