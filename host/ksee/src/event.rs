//! Turning probe hits into event records (currently JSON lines).

use std::fmt::Write;

use crate::tables::Tables;

/// Read access to kernel memory.
pub trait Memory {
    fn read(&self, addr: u32, buf: &mut [u8]);

    fn read_u32(&self, addr: u32) -> u32 {
        let mut b = [0; 4];
        self.read(addr, &mut b);
        u32::from_le_bytes(b)
    }
}

pub struct Recorder {
    tables: Tables,
    /// Address of the kernel's `__current_task`.
    current_task: u32,
}

impl Recorder {
    pub fn new(tables: Tables, current_task: u32) -> Recorder {
        Recorder { tables, current_task }
    }

    /// `{"pid":1,"comm":"init"}` for a task_struct pointer, else the number.
    fn task(&self, mem: &dyn Memory, p: u32, out: &mut String) {
        let (Some(pid), Some(comm)) = (self.tables.field("task_struct", "pid"),
                                       self.tables.field("task_struct", "comm"))
        else {
            let _ = write!(out, "{p}");
            return;
        };
        if p == 0 {
            out.push_str("null");
            return;
        }
        let mut name = vec![0; comm.size as usize];
        mem.read(p + comm.offset, &mut name);
        let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
        let _ = write!(out, "{{\"pid\":{},\"comm\":", mem.read_u32(p + pid.offset) as i32);
        json_string(&String::from_utf8_lossy(&name[..len]), out);
        out.push('}');
    }

    fn start(&self, mem: &dyn Memory, t: u64, kind: &str, func: u32) -> String {
        let mut out = String::with_capacity(128);
        let _ = write!(out, "{{\"t\":{t},\"kind\":\"{kind}\",\"fn\":");
        match self.tables.functions.get(&func) {
            Some(f) => json_string(&f.name, &mut out),
            None => {
                let _ = write!(out, "{func}");
            }
        }
        out.push_str(",\"cur\":");
        self.task(mem, mem.read_u32(self.current_task), &mut out);
        out
    }

    pub fn enter(&self, mem: &dyn Memory, t: u64, func: u32, args: [u32; 4]) -> String {
        let mut out = self.start(mem, t, "enter", func);
        let types = self.tables.functions.get(&func).map(|f| &f.arg_types[..]).unwrap_or(&[]);
        out.push_str(",\"args\":[");
        for (i, &a) in args.iter().enumerate().take(types.len().max(1).min(4)) {
            if i > 0 {
                out.push(',');
            }
            match types.get(i).map(String::as_str) {
                Some("task_struct*") => self.task(mem, a, &mut out),
                _ => {
                    let _ = write!(out, "{a}");
                }
            }
        }
        out.push_str("]}");
        out
    }

    pub fn exit(&self, mem: &dyn Memory, t: u64, func: u32, ret: u32) -> String {
        let mut out = self.start(mem, t, "exit", func);
        let _ = write!(out, ",\"ret\":{}}}", ret as i32);
        out
    }

    pub fn block(&self, mem: &dyn Memory, t: u64, func: u32, n: u32) -> String {
        let mut out = self.start(mem, t, "block", func);
        let _ = write!(out, ",\"n\":{n},\"loc\":");
        match self.tables.blocks.get(&(func, n)) {
            Some(loc) => json_string(loc, &mut out),
            None => out.push_str("null"),
        }
        out.push('}');
        out
    }
}

fn json_string(s: &str, out: &mut String) {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake kernel: one task_struct at 0x1000, __current_task at 0x10.
    struct Fake(Vec<u8>);

    impl Memory for Fake {
        fn read(&self, addr: u32, buf: &mut [u8]) {
            let a = addr as usize;
            buf.copy_from_slice(&self.0[a..a + buf.len()]);
        }
    }

    fn fake() -> (Recorder, Fake) {
        let tables = Tables::parse(
            "F\t1\tcontext_switch\tcore.c:5\trq*,task_struct*\nB\t1\t3\t9:2\n",
            "task_struct\tpid\t4\t4\ntask_struct\tcomm\t8\t16\n",
        );
        let mut mem = vec![0; 0x2000];
        mem[0x10..0x14].copy_from_slice(&0x1000u32.to_le_bytes());
        mem[0x1004..0x1008].copy_from_slice(&7u32.to_le_bytes());
        mem[0x1008..0x100c].copy_from_slice(b"ping");
        (Recorder::new(tables, 0x10), Fake(mem))
    }

    #[test]
    fn decodes_task_arguments() {
        let (r, mem) = fake();
        assert_eq!(
            r.enter(&mem, 5, 1, [0x42, 0x1000, 0, 0]),
            r#"{"t":5,"kind":"enter","fn":"context_switch","cur":{"pid":7,"comm":"ping"},"args":[66,{"pid":7,"comm":"ping"}]}"#
        );
    }

    #[test]
    fn blocks_carry_their_source_location() {
        let (r, mem) = fake();
        assert_eq!(
            r.block(&mem, 6, 1, 3),
            r#"{"t":6,"kind":"block","fn":"context_switch","cur":{"pid":7,"comm":"ping"},"n":3,"loc":"9:2"}"#
        );
    }
}
