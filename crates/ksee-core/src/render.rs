//! Rendering decoded traces as text or JSON.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt::Write;

use crate::cfg::Coverage;
use crate::dump::Task;
use crate::syscalls::Syscall;
use crate::tables::Tables;
use crate::trace::Record;

pub struct Renderer<'a> {
    pub tables: &'a Tables,
    /// A task_struct pointer's pid and comm (from a dump, or live memory).
    pub tasks: &'a dyn Fn(u32) -> Option<Task>,
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
        match (self.tasks)(p) {
            Some(t) => format!("{}({})", t.comm, t.pid),
            None => format!("{p:#x}"),
        }
    }

    fn task_json(&self, p: u32, out: &mut String) {
        match (self.tasks)(p) {
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

impl Renderer<'_> {
    fn cfg_header(&self, func: u32) -> String {
        let f = &self.tables.functions[&func];
        format!("{} {}:{}", f.name, self.tables.file(f.loc), f.loc.line)
    }

    /// One line per block: where it is, how often it ran, where it goes.
    pub fn cfg_text(&self, func: u32, cov: &Coverage) -> String {
        let blocks: Vec<(u32, _)> = self.tables.blocks_of(func).collect();
        let ran = blocks.iter().filter(|(n, _)| cov.block(func, *n) > 0).count();
        let mut out = format!("{}  ({} blocks, {} ran)\n", self.cfg_header(func), blocks.len(), ran);
        for (n, l) in blocks {
            let succs: Vec<String> = self.tables.successors.get(&(func, n)).into_iter().flatten()
                .map(|&s| match cov.edge(func, n, s) {
                    0 => format!("b{s}"),
                    c => format!("b{s}({c})"),
                }).collect();
            let _ = writeln!(out, "  {} b{:<4} {:>6}x  {:<40} -> {}",
                if cov.block(func, n) > 0 { '*' } else { ' ' }, n, cov.block(func, n),
                format!("{}:{}:{}", self.tables.file(l), l.line, l.column), succs.join(" "));
        }
        out
    }

    /// Graphviz: blocks that ran are filled, edges are labelled with counts.
    pub fn cfg_dot(&self, func: u32, cov: &Coverage) -> String {
        let mut out = String::from("digraph cfg {\n  node [shape=box fontname=monospace fontsize=10];\n");
        let _ = writeln!(out, "  label=\"{}\";", self.cfg_header(func));
        for (n, l) in self.tables.blocks_of(func) {
            let hits = cov.block(func, n);
            let style = if hits > 0 { " style=filled fillcolor=\"#ffd27f\"" } else { "" };
            let _ = writeln!(out, "  b{n} [label=\"b{n} ({hits}x)\\n{}:{}\"{style}];",
                             self.tables.file(l), l.line);
            for &s in self.tables.successors.get(&(func, n)).into_iter().flatten() {
                match cov.edge(func, n, s) {
                    0 => { let _ = writeln!(out, "  b{n} -> b{s} [color=gray];"); }
                    c => { let _ = writeln!(out, "  b{n} -> b{s} [label=\"{c}\" penwidth=2];"); }
                }
            }
        }
        out.push_str("}\n");
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

/// A type's layout, pahole style: each member with its offset and size.
pub fn layout(types: &crate::types::Types, id: u32) -> String {
    use crate::types::Kind;
    let t = types.get(types.resolve(id));
    let mut out = format!("{} {{\t/* size {} */\n", types.name(types.resolve(id)), t.size);
    match &t.kind {
        Kind::Struct(members) | Kind::Union(members) => {
            for m in members {
                let decl = format!("{} {}", types.name(m.ty), m.name);
                let bits = if m.bit_size > 0 { format!(":{}", m.bit_size) } else { String::new() };
                let _ = writeln!(out, "\t{:<48}\t/* {:>5} {:>5} */", decl + &bits + ";",
                                 m.bit_offset / 8, types.size_of(m.ty));
            }
        }
        Kind::Enum(values) => {
            for (name, v) in values {
                let _ = writeln!(out, "\t{name} = {v},");
            }
        }
        _ => {}
    }
    out.push_str("};\n");
    out
}
