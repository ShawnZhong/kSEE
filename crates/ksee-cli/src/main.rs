//! The ksee command (./ksee.sh runs it, in the kSEE tree):
//!
//!   ksee                     build, then serve the page
//!   ksee build               build the kernel, runtime and page (build.rs)
//!   ksee test [--ui]         build, then run the tests (--ui: in Chromium)
//!   ksee deploy              build, then push the page to origin's gh-pages
//!   ksee pack KERNEL OUT     prepare the kernel tree KERNEL (built) for the
//!                            page, in OUT: OUT/vmlinux, with the kernel's
//!                            types and syscalls from DWARF, without DWARF;
//!                            and OUT/src, the sources its probes refer to
//!   ksee serve [DIR] [--port=N]
//!                            serve DIR (target/site: the page, as built) for
//!                            a browser
//!   ksee type VMLINUX NAME   the layout of a kernel struct, union, enum or
//!                            typedef
mod build;
mod deploy;
mod pack;
mod serve;

use std::path::Path;
use std::process::ExitCode;

use ksee_core::{module, types::Types};

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
        ["pack", kernel, out] => pack::pack(Path::new(kernel), Path::new(out)),
        ["serve", ref dir @ ..] if dir.len() <= 1 => {
            let dir = Path::new(dir.first().copied().unwrap_or(build::SITE));
            serve::serve(dir, port).unwrap_or_else(|e| fail(&format!("serve: {e}")));
        }
        ["type", vmlinux, name] => {
            let image = read(Path::new(vmlinux));
            let types = Types::parse(&String::from_utf8_lossy(&module::custom_section(&image, "ksee.types")));
            let Some(id) = types.find(name) else { fail(&format!("{name}: no such type")) };
            print!("{}", types.layout(id));
        }
        _ => {
            eprintln!("usage: ksee [build | test [--ui] | deploy | serve [DIR] [--port=N] | pack KERNEL OUT | type VMLINUX NAME]");
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}
