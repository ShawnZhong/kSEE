//! Rendering decoded traces as text or JSON.

use std::collections::BTreeMap;
use std::fmt::Write;

use ksee_core::dump::Task;
use ksee_core::syscalls::Syscall;
use ksee_core::tables::Tables;
use ksee_core::trace::Record;

pub struct Renderer<'a> {
    pub tables: &'a Tables,
    pub tasks: &'a BTreeMap<u32, Task>,
}

impl Renderer<'_> {
    pub fn func(&self, id: u32) -> String {
        self.tables.functions.get(&id).map_or_else(|| format!("{id:#x}"), |f| f.name.clone())
    }

    /// "file:line:column" of a block (files are relative to the kernel tree).
    pub fn block_loc(&self, func: u32, n: u32) -> String {
        match self.tables.blocks.get(&(func, n)) {
            Some(&l) => format!("{}:{}:{}", self.tables.file(l), l.line, l.column),
            None => "?".into(),
        }
    }

    pub fn task(&self, p: u32) -> String {
        match self.tasks.get(&p) {
            Some(t) => format!("{}({})", t.comm, t.pid),
            None => format!("{p:#x}"),
        }
    }

    fn task_json(&self, p: u32, out: &mut String) {
        match self.tasks.get(&p) {
            Some(t) => {
                let _ = write!(out, "{{\"pid\":{},\"comm\":", t.pid);
                string(&t.comm, out);
                out.push('}');
            }
            None => {
                let _ = write!(out, "{p}");
            }
        }
    }

    fn arg_types(&self, func: u32) -> &[String] {
        self.tables.functions.get(&func).map_or(&[], |f| &f.arg_types[..])
    }

    pub fn record_text(&self, r: &Record) -> String {
        match *r {
            Record::Block { func, n } => {
                format!("    {:<24} {}", self.func(func), self.block_loc(func, n))
            }
            Record::Enter { func, time, current, args } => {
                let types = self.arg_types(func);
                let args: Vec<String> = args.iter().take(types.len()).zip(types).map(|(&a, t)| {
                    if t == "task_struct*" { self.task(a) } else { format!("{a:#x}") }
                }).collect();
                format!("{} {} > {}({})", secs(time), self.task(current), self.func(func), args.join(", "))
            }
            Record::Exit { func, time, current, value } => {
                format!("{} {} < {} = {}", secs(time), self.task(current), self.func(func), value as i32)
            }
        }
    }

    pub fn record_json(&self, r: &Record) -> String {
        let mut out = String::new();
        match *r {
            Record::Block { func, n } => {
                out.push_str("{\"kind\":\"block\",\"fn\":");
                string(&self.func(func), &mut out);
                out.push_str(",\"at\":");
                string(&self.block_loc(func, n), &mut out);
            }
            Record::Enter { func, time, current, args } => {
                let _ = write!(out, "{{\"kind\":\"enter\",\"t\":{time},\"fn\":");
                string(&self.func(func), &mut out);
                out.push_str(",\"cur\":");
                self.task_json(current, &mut out);
                out.push_str(",\"args\":[");
                let types = self.arg_types(func);
                for (i, (&a, t)) in args.iter().zip(types).enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    if t == "task_struct*" {
                        self.task_json(a, &mut out);
                    } else {
                        let _ = write!(out, "{a}");
                    }
                }
                out.push(']');
            }
            Record::Exit { func, time, current, value } => {
                let _ = write!(out, "{{\"kind\":\"exit\",\"t\":{time},\"fn\":");
                string(&self.func(func), &mut out);
                out.push_str(",\"cur\":");
                self.task_json(current, &mut out);
                let _ = write!(out, ",\"ret\":{}", value as i32);
            }
        }
        out.push('}');
        out
    }

    pub fn syscall_text(&self, s: &Syscall, blocks: bool) -> String {
        let ret = s.ret.map_or("?".into(), |r| r.to_string());
        let dur = s.end.map_or("?".into(), |e| format!("{}us", (e - s.start) / 1000));
        let mut funcs: Vec<u32> = s.blocks.iter().map(|b| b.0).collect();
        funcs.sort();
        funcs.dedup();
        let mut out = format!(
            "{} {} syscall {}({:#x}, {:#x}, {:#x}) = {}  [{}, {} blocks in {} functions{}]",
            secs(s.start), self.task(s.task), s.nr, s.args[0], s.args[1], s.args[2], ret, dur,
            s.blocks.len(), funcs.len(), if s.partial { ", partial" } else { "" });
        if blocks {
            for &(func, n) in &s.blocks {
                let _ = write!(out, "\n    {:<24} {}", self.func(func), self.block_loc(func, n));
            }
        }
        out
    }

    pub fn syscall_json(&self, s: &Syscall) -> String {
        let mut out = String::from("{\"task\":");
        self.task_json(s.task, &mut out);
        let _ = write!(out, ",\"nr\":{},\"args\":[{},{},{}],\"start\":{}", s.nr,
                       s.args[0], s.args[1], s.args[2], s.start);
        match (s.end, s.ret) {
            (Some(end), Some(ret)) => {
                let _ = write!(out, ",\"end\":{end},\"ret\":{ret}");
            }
            _ => out.push_str(",\"end\":null,\"ret\":null"),
        }
        let _ = write!(out, ",\"partial\":{},\"blocks\":[", s.partial);
        for (i, &(func, n)) in s.blocks.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push('[');
            string(&self.func(func), &mut out);
            out.push(',');
            string(&self.block_loc(func, n), &mut out);
            out.push(']');
        }
        out.push_str("]}");
        out
    }
}

fn secs(ns: u64) -> String {
    format!("[{:>5}.{:06}]", ns / 1_000_000_000, ns / 1000 % 1_000_000)
}

fn string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}
