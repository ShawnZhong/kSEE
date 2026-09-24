//! JSON output (hand-written: the host has no serde).

use alloc::string::String;
use core::fmt::Write;

use crate::syscalls::Syscall;
use crate::tables::Tables;
use crate::trace::Record;
use crate::Memory;

pub struct Writer<'a> {
    pub tables: &'a Tables,
    pub mem: &'a dyn Memory,
}

impl Writer<'_> {
    /// `{"pid":1,"comm":"init"}` for a task_struct pointer.
    pub fn task(&self, p: u32, out: &mut String) {
        let t = self.tables;
        let (Some(pid), Some(comm)) = (t.field("task_struct", "pid"), t.field("task_struct", "comm"))
        else {
            let _ = write!(out, "{p}");
            return;
        };
        if p == 0 {
            out.push_str("null");
            return;
        }
        let mut name = alloc::vec![0; comm.size as usize];
        self.mem.read(p + comm.offset, &mut name);
        let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
        let _ = write!(out, "{{\"pid\":{},\"comm\":", self.mem.read_u32(p + pid.offset) as i32);
        string(&String::from_utf8_lossy(&name[..len]), out);
        out.push('}');
    }

    fn func(&self, func: u32, out: &mut String) {
        match self.tables.functions.get(&func) {
            Some(f) => string(&f.name, out),
            None => {
                let _ = write!(out, "{func}");
            }
        }
    }

    /// A block as `["function","file:line:column"]`.
    fn block(&self, func: u32, n: u32, out: &mut String) {
        out.push('[');
        self.func(func, out);
        out.push(',');
        match self.tables.blocks.get(&(func, n)) {
            Some(&l) => {
                let _ = write!(out, "\"{}:{}:{}\"", self.tables.file(l), l.line, l.column);
            }
            None => out.push_str("null"),
        }
        out.push(']');
    }

    pub fn record(&self, r: &Record) -> String {
        let mut out = String::with_capacity(96);
        match *r {
            Record::Block { func, n } => {
                out.push_str("{\"kind\":\"block\",\"at\":");
                self.block(func, n, &mut out);
            }
            Record::Enter { func, time, current, args } => {
                let _ = write!(out, "{{\"kind\":\"enter\",\"t\":{time},\"fn\":");
                self.func(func, &mut out);
                out.push_str(",\"cur\":");
                self.task(current, &mut out);
                out.push_str(",\"args\":[");
                let types = self.tables.functions.get(&func).map(|f| &f.arg_types[..]).unwrap_or(&[]);
                for (i, &a) in args.iter().enumerate().take(types.len().clamp(1, 4)) {
                    if i > 0 {
                        out.push(',');
                    }
                    if types.get(i).map(String::as_str) == Some("task_struct*") {
                        self.task(a, &mut out);
                    } else {
                        let _ = write!(out, "{a}");
                    }
                }
                out.push(']');
            }
            Record::Exit { func, time, current, value } => {
                let _ = write!(out, "{{\"kind\":\"exit\",\"t\":{time},\"fn\":");
                self.func(func, &mut out);
                out.push_str(",\"cur\":");
                self.task(current, &mut out);
                let _ = write!(out, ",\"ret\":{}", value as i32);
            }
        }
        out.push('}');
        out
    }

    pub fn syscall(&self, s: &Syscall) -> String {
        let mut out = String::with_capacity(64 + s.blocks.len() * 48);
        out.push_str("{\"task\":");
        self.task(s.task, &mut out);
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
            self.block(func, n, &mut out);
        }
        out.push_str("]}");
        out
    }
}

pub fn string(s: &str, out: &mut String) {
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
