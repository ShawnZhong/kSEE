//! The ksee command (./ksee.sh runs it, in the kSEE tree):
//!
//!   ksee                     build, then serve the page
//!   ksee build               build the kernel, runtime and page (build.rs)
//!   ksee test [--ui]         build, then run the tests (in Chromium,
//!                            $CHROME; --ui: the page's too)
//!   ksee shot OUT.png [--test=NAME | --eval=JS] [--at=#HASH] [--url=URL]
//!                     [--wait=MS] [--size=WxH]
//!                            the page in Chromium, after a script (or one
//!                            of the tests), as a screenshot
//!   ksee deploy              build, then push the page to origin's gh-pages
//!   ksee pack KERNEL OUT     prepare the kernel tree KERNEL (built) for the
//!                            page, in OUT: OUT/vmlinux, with the kernel's
//!                            types and syscalls from DWARF, without DWARF;
//!                            and OUT/src, the sources its probes refer to
//!   ksee patch               write linux.patch from linux/ (the port, as it
//!                            differs from the release)
//!   ksee serve [DIR] [--port=N]
//!                            serve DIR (target/site: the page, as built) for
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
    let opt = |k: &str| args.iter().find_map(|a| a.strip_prefix(&format!("--{k}=")).map(String::from));
    let args: Vec<&str> = args.iter().filter(|a| !a.starts_with("--")).map(String::as_str).collect();
    match args[..] {
        [] => {
            build::build();
            serve::serve(Path::new(build::SITE), port).unwrap_or_else(|e| fail(&format!("serve: {e}")));
        }
        ["build"] => build::build(),
        ["test"] => build::test(ui),
        ["deploy"] => deploy::deploy(),
        ["shot", out] => {
            let out = Some(Path::new(out).to_path_buf());
            let mut shot = match opt("test") {
                Some(name) => chrome::Shot::test(&name, out),
                None => chrome::Shot { site: build::SITE.into(), url: None, hash: String::new(), size: (1500, 900), wait: 6000, eval: None, out },
            };
            shot.hash = opt("at").unwrap_or(shot.hash);
            shot.url = opt("url");
            shot.eval = opt("eval").or(shot.eval);
            shot.wait = opt("wait").and_then(|w| w.parse().ok()).unwrap_or(shot.wait);
            if let Some((w, h)) = opt("size").and_then(|s| Some((s.split_once('x')?.0.parse().ok()?, s.split_once('x')?.1.parse().ok()?))) {
                shot.size = (w, h);
            }
            if !shot.run() {
                return ExitCode::FAILURE;
            }
        }
        ["pack", kernel, out] => pack::pack(Path::new(kernel), Path::new(out)),
        ["patch"] => build::patch(),
        ["serve", ref dir @ ..] if dir.len() <= 1 => {
            let dir = Path::new(dir.first().copied().unwrap_or(build::SITE));
            serve::serve(dir, port).unwrap_or_else(|e| fail(&format!("serve: {e}")));
        }
        _ => {
            eprintln!("usage: ksee [build | test [--ui] | shot OUT.png [--test=NAME | --eval=JS] [--at=#HASH] [--url=URL] [--wait=MS] [--size=WxH] \
                       | deploy | serve [DIR] [--port=N] | pack KERNEL OUT | patch]");
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}
