//! kSEE command line: read a trace dump offline.
//!
//!   ksee records  VMLINUX DUMP [--json]            every record
//!   ksee syscalls VMLINUX DUMP [--json] [--blocks]  what each syscall ran
//!
//! VMLINUX (the kernel module) provides the probe tables; DUMP is written
//! by the host (node host/node.js --dump FILE).
mod render;

use std::io::{BufWriter, Write};
use std::process::ExitCode;

use ksee_core::dump::Dump;
use ksee_core::tables::Tables;
use ksee_core::{module, syscalls, trace};

use render::Renderer;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flags: Vec<&str> = args.iter().filter(|a| a.starts_with("--")).map(String::as_str).collect();
    let pos: Vec<&str> = args.iter().filter(|a| !a.starts_with("--")).map(String::as_str).collect();
    let [cmd, vmlinux, dump] = pos[..] else {
        eprintln!("usage: ksee records|syscalls VMLINUX DUMP [--json] [--blocks]");
        return ExitCode::from(2);
    };
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
    let r = Renderer { tables: &tables, tasks: &dump.tasks };
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
            let Some(entry) = tables.find(syscalls::ENTRY) else {
                eprintln!("{vmlinux}: {} is not traced", syscalls::ENTRY);
                return ExitCode::FAILURE;
            };
            for s in syscalls::split(entry, trace::decode(&dump.words)) {
                let line = if json { r.syscall_json(&s) } else { r.syscall_text(&s, flags.contains(&"--blocks")) };
                let _ = writeln!(out, "{line}");
            }
        }
        _ => {
            eprintln!("unknown command {cmd}");
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}
