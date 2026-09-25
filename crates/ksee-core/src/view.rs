//! Structured views of a trace, for UIs (serializable with the "serde"
//! feature).

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use alloc::collections::BTreeMap;

use crate::task::Task;
use crate::syscalls::Syscall;
use crate::tables::Tables;
use crate::types::{Base, Kind, Types};
use crate::Memory;

#[cfg(feature = "serde")]
use serde::Serialize;

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct TaskRef {
    pub addr: u32,
    pub pid: Option<i32>,
    pub comm: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct SyscallSummary {
    pub index: usize,
    pub task: TaskRef,
    pub nr: u32,
    /// From the syscall table, else the handler that ran ("write" for
    /// __wasm_sys_write).
    pub name: Option<String>,
    pub args: [u32; 3],
    pub ret: Option<i32>,
    pub start_ns: u64,
    pub duration_ns: Option<u64>,
    pub blocks: usize,
    pub functions: usize,
    pub partial: bool,
    /// init's own traffic on the /dev/ksee channel (fetching a request,
    /// replying), not a syscall the page made.
    pub control: bool,
    /// Its span in the record stream (for timelines).
    pub first: usize,
    pub last: Option<usize>,
}

/// Consecutive blocks of one function.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Run {
    pub func: String,
    pub blocks: usize,
    /// Where the function is defined.
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Block {
    pub n: u32,
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Function {
    pub name: String,
    pub file: String,
    pub line: u32,
    pub blocks: Vec<Block>,
}

/// A value in memory. Aggregates beyond the requested depth have no
/// children yet (`expand` names the type to ask for them).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Object {
    pub name: String,
    pub ty: String,
    pub addr: u32,
    pub value: Option<String>,
    pub children: Option<Vec<Object>>,
    /// Type id to expand this value (an aggregate not expanded yet).
    pub expand: Option<u32>,
    /// For a pointer to a struct or union: the target's type id.
    pub target: Option<u32>,
}

pub struct Views<'a> {
    pub tables: &'a Tables,
    /// The task at a task_struct address, at a record (the address may be
    /// another task's later).
    pub tasks: &'a dyn Fn(u32, usize) -> Option<Task>,
}

impl Views<'_> {
    fn task(&self, addr: u32, at: usize) -> TaskRef {
        let t = (self.tasks)(addr, at);
        TaskRef { addr, pid: t.as_ref().map(|t| t.pid), comm: t.map(|t| t.comm) }
    }

    fn func_name(&self, id: u32) -> String {
        self.tables.functions.get(&id).map_or_else(|| format!("{id:#x}"), |f| f.name.clone())
    }

    pub fn summary(&self, index: usize, s: &Syscall) -> SyscallSummary {
        let mut funcs: Vec<u32> = s.blocks.iter().map(|b| b.0).collect();
        funcs.sort();
        funcs.dedup();
        let name = self.tables.types.syscalls.get(&s.nr).map(|c| c.name.clone()).or_else(|| {
            s.blocks.iter().find_map(|&(f, _)| {
                self.tables.functions.get(&f)?.name.strip_prefix("__wasm_sys_").map(String::from)
            })
        });
        let control = funcs.iter().any(|f| {
            self.tables.functions.get(f).is_some_and(|f| f.name.starts_with("ksee_channel_"))
        });
        SyscallSummary {
            control,
            first: s.first,
            last: s.last,
            index,
            task: self.task(s.task, s.first),
            nr: s.nr,
            name,
            args: s.args,
            ret: s.ret,
            start_ns: s.start,
            duration_ns: s.end.map(|e| e - s.start),
            blocks: s.blocks.len(),
            functions: funcs.len(),
            partial: s.partial,
        }
    }

    /// The syscall's execution as runs of blocks per function.
    pub fn runs(&self, s: &Syscall) -> Vec<Run> {
        let mut runs: Vec<(u32, Run)> = Vec::new();
        for &(f, n) in &s.blocks {
            match runs.last_mut() {
                Some((last, run)) if *last == f => run.blocks += 1,
                _ => {
                    let (file, line) = match self.tables.functions.get(&f) {
                        Some(func) => (self.tables.file(func.loc).to_string(), func.loc.line),
                        None => (String::new(), 0),
                    };
                    let _ = n;
                    runs.push((f, Run { func: self.func_name(f), blocks: 1, file, line }));
                }
            }
        }
        runs.into_iter().map(|(_, r)| r).collect()
    }

    /// Function `id`'s blocks, with how often each ran (`counts`:
    /// (function, block) -> times).
    pub fn function(&self, id: u32, counts: &BTreeMap<(u32, u32), u64>) -> Option<Function> {
        let f = self.tables.functions.get(&id)?;
        let blocks = self.tables.blocks_of(id).map(|(n, l)| Block {
            n,
            file: self.tables.file(l).to_string(),
            line: l.line,
            column: l.column,
            count: counts.get(&(id, n)).copied().unwrap_or(0),
        }).collect();
        Some(Function { name: f.name.clone(), file: self.tables.file(f.loc).to_string(), line: f.loc.line, blocks })
    }
}

impl Types {
    /// The value of type `ty` at `addr`, aggregates expanded `depth` deep.
    pub fn object(&self, mem: &dyn Memory, name: &str, addr: u32, ty: u32, depth: u32) -> Object {
        self.object_bits(mem, name, addr, ty, 0, 0, depth)
    }

    #[allow(clippy::too_many_arguments)]
    fn object_bits(&self, mem: &dyn Memory, name: &str, addr: u32, ty: u32, bit_off: u32,
                   bit_size: u32, depth: u32) -> Object {
        let id = self.resolve(ty);
        let t = self.get(id);
        let mut o = Object {
            name: name.to_string(), ty: self.name(ty), addr,
            value: None, children: None, expand: None, target: None,
        };
        match &t.kind {
            Kind::Struct(members) | Kind::Union(members) => {
                if depth == 0 {
                    o.expand = Some(id);
                } else {
                    o.children = Some(members.iter().map(|m| {
                        let (boff, bsize) = if m.bit_size > 0 { (m.bit_offset % 8, m.bit_size) } else { (0, 0) };
                        self.object_bits(mem, &m.name, addr + m.bit_offset / 8, m.ty, boff, bsize, depth - 1)
                    }).collect());
                }
            }
            Kind::Array { elem, count } if !self.is_string(*elem) => {
                if depth == 0 {
                    o.expand = Some(id);
                } else {
                    let esize = self.size_of(*elem);
                    o.children = Some((0..(*count).min(64)).map(|i| {
                        self.object_bits(mem, &format!("[{i}]"), addr + i * esize, *elem, 0, 0, depth - 1)
                    }).collect());
                }
            }
            Kind::Pointer(to) => {
                let p = mem.read_u32(addr);
                o.value = Some(if p == 0 { "NULL".into() } else { format!("{p:#x}") });
                let target = self.resolve(*to);
                if p != 0 && matches!(self.get(target).kind, Kind::Struct(_) | Kind::Union(_)) {
                    o.target = Some(target);
                }
            }
            _ => {
                let mut v = String::new();
                self.print_bits(mem, addr, ty, bit_off, bit_size, &mut v);
                o.value = Some(v);
            }
        }
        o
    }

    fn is_string(&self, elem: u32) -> bool {
        matches!(self.get(self.resolve(elem)).kind, Kind::Base(Base::Char)) && self.size_of(elem) == 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Mem(Vec<u8>);
    impl Memory for Mem {
        fn read(&self, addr: u32, buf: &mut [u8]) {
            buf.copy_from_slice(&self.0[addr as usize..addr as usize + buf.len()]);
        }
    }

    #[test]
    fn objects_expose_pointer_targets() {
        let t = Types::parse("T\t1\tbase\tint\t4\ts\nT\t2\tstruct\tnode\t8\nM\t2\tv\t0\t0\t1\n\
                              M\t2\tnext\t32\t0\t3\nT\t3\tptr\t\t4\t2\n");
        let mut m = alloc::vec![0u8; 16];
        m[0..4].copy_from_slice(&7i32.to_le_bytes());
        m[4..8].copy_from_slice(&8u32.to_le_bytes());
        let o = t.object(&Mem(m), "n", 0, 2, 1);
        let c = o.children.unwrap();
        assert_eq!((c[0].value.as_deref(), c[1].value.as_deref(), c[1].target), (Some("7"), Some("0x8"), Some(2)));
    }
}
