//! The ksee command (./ksee.sh runs it, in the kSEE tree):
//!
//!   ksee                     build, then serve the page
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

fn read(p: &Path) -> Vec<u8> {
    std::fs::read(p).unwrap_or_else(|e| fail(&format!("{}: {e}", p.display())))
}

fn fail(msg: &str) -> ! {
    eprintln!("ksee: {msg}");
    std::process::exit(1)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let port = args.iter().find_map(|a| a.strip_prefix("--port=")?.parse().ok()).unwrap_or(8000);
    let ui = args.iter().any(|a| a == "--ui");
    let args: Vec<&str> = args.iter().filter(|a| !a.starts_with("--")).map(String::as_str).collect();
    match args[..] {
        [] => {
            build::build();
            serve::serve(Path::new(build::SITE), port).unwrap_or_else(|e| fail(&format!("serve: {e}")));
        }
        ["build"] => build::build(),
        ["test"] => build::test(ui),
        ["deploy"] => deploy::deploy(),
        ["patch"] => build::patch(),
        ["serve"] => {
            serve::serve(Path::new(build::SITE), port).unwrap_or_else(|e| fail(&format!("serve: {e}")));
        }
        _ => {
            eprintln!("usage: ksee [build | test [--ui] | deploy | serve [--port=N] | patch]");
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}
