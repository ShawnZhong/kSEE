//! kSEE command line: read a trace dump offline.
//!
//!   ksee records  VMLINUX DUMP [--json]            every record
//!   ksee syscalls VMLINUX DUMP [--json] [--blocks]  what each syscall ran
//!   ksee serve [ROOT] [--port=N]  serve the web host (ROOT: the kSEE tree)
//!   ksee cfg VMLINUX DUMP FUNCTION [--dot] [--syscall=N]
//!                 FUNCTION's control flow graph, with how often each block
//!                 and edge ran (in the whole trace, or in syscall N as
//!                 numbered by `ksee syscalls`)
//!
//! VMLINUX (the kernel module) provides the probe tables; DUMP is written
//! by the host (node host/node.js --dump FILE).
use std::io::{BufWriter, Write};
use std::process::ExitCode;

use ksee_core::dump::Dump;
use ksee_core::tables::Tables;
use ksee_core::syscalls::Syscall;
use ksee_core::{cfg, module, syscalls, trace};

use ksee_core::render::Renderer;

mod serve;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flags: Vec<&str> = args.iter().filter(|a| a.starts_with("--")).map(String::as_str).collect();
    let pos: Vec<&str> = args.iter().filter(|a| !a.starts_with("--")).map(String::as_str).collect();
    if pos.first() == Some(&"serve") {
        let root = std::path::Path::new(pos.get(1).copied().unwrap_or("."));
        let port = flags.iter().find_map(|f| f.strip_prefix("--port=")?.parse().ok()).unwrap_or(8000);
        return match serve::serve(root, port) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("serve: {e}");
                ExitCode::FAILURE
            }
        };
    }
    let (cmd, vmlinux, dump, rest) = match pos[..] {
        [cmd, vmlinux, dump, ref rest @ ..] => (cmd, vmlinux, dump, rest),
        _ => {
            eprintln!("usage: ksee records|syscalls|cfg VMLINUX DUMP ... (see crates/ksee-cli)");
            return ExitCode::from(2);
        }
    };
    let flag = |name: &str| flags.iter().find_map(|f| f.strip_prefix(name)?.strip_prefix('='));
    let read = |p: &str| std::fs::read(p).unwrap_or_else(|e| panic!("{p}: {e}"));
    let image = read(vmlinux);
    let text = |name| String::from_utf8_lossy(&module::custom_section(&image, name)).into_owned();
    let tables = Tables::parse(&text("ksee.probes"), &text("ksee.types"));
    let dump = match Dump::parse(&read(dump)) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{dump}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let task = |p| dump.tasks.get(&p).cloned();
    let r = Renderer { tables: &tables, tasks: &task };
    let json = flags.contains(&"--json");
    let mut out = BufWriter::new(std::io::stdout().lock());
    match cmd {
        "records" => {
            for rec in trace::decode(&dump.words) {
                let line = if json { r.record_json(&rec) } else { r.record_text(&rec) };
                let _ = writeln!(out, "{line}");
            }
        }
        "syscalls" => {
            let Some(calls) = split_syscalls(&tables, &dump, vmlinux) else {
                return ExitCode::FAILURE;
            };
            for (i, s) in calls.iter().enumerate() {
                let line = if json { r.syscall_json(s) } else {
                    format!("#{i} {}", r.syscall_text(s, flags.contains(&"--blocks")))
                };
                let _ = writeln!(out, "{line}");
            }
        }
        "cfg" => {
            let [name] = rest[..] else {
                eprintln!("usage: ksee cfg VMLINUX DUMP FUNCTION [--dot] [--syscall=N]");
                return ExitCode::from(2);
            };
            let ids: Vec<u32> = tables.functions.iter()
                .filter(|(_, f)| f.name == name).map(|(&id, _)| id).collect();
            if ids.is_empty() {
                eprintln!("{name}: no such traced function");
                return ExitCode::FAILURE;
            }
            let records: Vec<trace::Record> = match flag("--syscall") {
                Some(n) => {
                    let Some(calls) = split_syscalls(&tables, &dump, vmlinux) else {
                        return ExitCode::FAILURE;
                    };
                    let Some(call) = n.parse::<usize>().ok().and_then(|n| calls.into_iter().nth(n)) else {
                        eprintln!("--syscall={n}: no such syscall");
                        return ExitCode::FAILURE;
                    };
                    call.blocks.iter().map(|&(func, n)| trace::Record::Block { func, n }).collect()
                }
                None => trace::decode(&dump.words),
            };
            let cov = cfg::coverage(&tables, records, |f| ids.contains(&f));
            for id in ids {
                let text = if flags.contains(&"--dot") { r.cfg_dot(id, &cov) } else { r.cfg_text(id, &cov) };
                let _ = write!(out, "{text}");
            }
        }
        _ => {
            eprintln!("unknown command {cmd}");
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}

fn split_syscalls(tables: &Tables, dump: &Dump, vmlinux: &str) -> Option<Vec<Syscall>> {
    let Some(entry) = tables.find(syscalls::ENTRY) else {
        eprintln!("{vmlinux}: {} is not traced", syscalls::ENTRY);
        return None;
    };
    Some(syscalls::split(entry, trace::decode(&dump.words)))
}
