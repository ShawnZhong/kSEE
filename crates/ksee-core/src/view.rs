//! Views of a recording, for the page (serializable with the "serde"
//! feature): what it shows, by name rather than by id.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use alloc::collections::BTreeMap;

use crate::recording::Recording;
use crate::stack::{self, LineStep};
use crate::tables::Tables;
use crate::task::read_task;
use crate::trace::Record;
use crate::types::{Base, Kind, Types};
use crate::Memory;

#[cfg(feature = "serde")]
use serde::Serialize;

/// Who a task was (pid and comm unknown if the recording does not say, and
/// the task_struct is another's by now).
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
    pub name: Option<String>,
    pub args: [u32; 5],
    /// The result (None: still running at the end of the recording).
    pub ret: Option<i32>,
    /// Its span in the records.
    pub first: usize,
    pub last: Option<usize>,
}

/// Where the kernel was at record `i`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct At {
    pub i: usize,
    /// "block", "return", or another record's kind.
    pub kind: &'static str,
    pub func: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub task: TaskRef,
    /// The syscall in progress on the task (its index), if any.
    pub syscall: Option<usize>,
}

/// A source line the kernel ran (see stack::lines), at record `i`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Line {
    pub i: usize,
    pub func: String,
    pub file: String,
    pub line: u32,
    /// The function the line is inlined from, if.
    pub inlined: Option<String>,
}

/// A program's run (see recording::Run).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Program {
    pub task: u32,
    pub first: usize,
    pub last: usize,
    pub lines: Vec<ProgramLine>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct ProgramLine {
    pub i: usize,
    pub line: u32,
}

/// A call on a stack, with its arguments (32 bits each: wider ones lose
/// their high half) and its locals in memory.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Frame {
    pub func: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub args: Option<Vec<Object>>,
    pub locals: Option<Vec<Object>>,
}

/// The newest write to a value: at record `i`, on `task`, by the code at
/// `func`'s `file:line` (`inlined` from another function, maybe); `old`
/// what it overwrote.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Write {
    pub i: usize,
    pub old: u64,
    pub task: TaskRef,
    pub func: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub inlined: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Block {
    pub n: u32,
    pub file: String,
    pub line: u32,
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

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct LineCount {
    pub line: u32,
    pub count: u64,
}

/// A value in memory. Aggregates beyond the requested depth have no
/// children yet (`expand` names the type to ask for them).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Object {
    pub name: String,
    pub ty: String,
    pub addr: u32,
    /// Its size in bytes.
    pub size: u32,
    pub value: Option<String>,
    pub children: Option<Vec<Object>>,
    /// Type id to expand this value (an aggregate not expanded yet).
    pub expand: Option<u32>,
    /// For a pointer to a struct or union: the target's type id.
    pub target: Option<u32>,
}

/// Views of a recording. `now`: memory as it is (for tasks the recording
/// does not name).
pub struct View<'a> {
    pub tables: &'a Tables,
    pub rec: &'a Recording,
    pub now: &'a dyn Memory,
}

/// Bytes, as if they were memory at address 0.
struct Bytes<'a>(&'a [u8]);

impl Memory for Bytes<'_> {
    fn read(&self, addr: u32, buf: &mut [u8]) {
        for (i, b) in buf.iter_mut().enumerate() {
            *b = self.0.get(addr as usize + i).copied().unwrap_or(0);
        }
    }
}

impl View<'_> {
    fn name(&self, func: u32) -> String {
        self.tables.functions.get(&func).map_or_else(|| format!("{func:#x}"), |f| f.name.clone())
    }

    /// Who the task at `addr` was at record `i`.
    pub fn task(&self, addr: u32, i: usize) -> TaskRef {
        let t = self.rec.task_at(addr, i).or_else(|| read_task(self.tables, self.now, addr));
        TaskRef { addr, pid: t.as_ref().map(|t| t.pid), comm: t.map(|t| t.comm) }
    }

    pub fn syscalls(&self) -> Vec<SyscallSummary> {
        self.rec.syscalls.iter().enumerate().map(|(index, s)| SyscallSummary {
            index,
            task: self.task(s.task, s.first),
            nr: s.nr,
            name: self.tables.types.syscalls.get(&s.nr).cloned(),
            args: s.args,
            ret: s.ret,
            first: s.first,
            last: s.last,
        }).collect()
    }

    pub fn at(&self, i: usize) -> Option<At> {
        let t = self.tables;
        let (kind, func, loc) = match *self.rec.records.get(i)? {
            Record::Block { func, n } => ("block", func, t.blocks.get(&(func, n)).copied()),
            Record::Return { func, .. } => ("return", func, t.functions.get(&func).map(|f| f.loc)),
            Record::Line { .. } => ("line", 0, None),
            Record::Task { .. } => ("task", 0, None),
            Record::Args { .. } => ("args", 0, None),
            Record::Store { .. } => ("store", 0, None),
            Record::Locals { .. } => ("locals", 0, None),
        };
        Some(At {
            i, kind,
            func: t.functions.get(&func).map_or(String::new(), |f| f.name.clone()),
            file: loc.map(|l| t.file(l).to_string()),
            line: loc.map(|l| l.line),
            task: self.task(self.rec.tasks[i], i),
            syscall: self.rec.syscall_at(i),
        })
    }

    fn line(&self, s: &LineStep) -> Line {
        let t = self.tables;
        Line { i: s.record, func: self.name(s.func), file: t.file(s.loc).to_string(), line: s.loc.line,
               inlined: s.inlined.map(|k| t.names.get(k as usize).cloned().unwrap_or_default()) }
    }

    /// The source lines run in records `first..=last` of the task at `task`.
    pub fn lines(&self, first: usize, last: usize, task: u32) -> Vec<Line> {
        self.rec.lines(self.tables, first, last, task).iter().map(|s| self.line(s)).collect()
    }

    pub fn program(&self, pid: i32) -> Option<Program> {
        let r = self.rec.program(pid)?;
        Some(Program { task: r.task, first: r.first, last: r.last,
                       lines: r.lines.into_iter().map(|(i, line)| ProgramLine { i, line }).collect() })
    }

    /// The call stack at record `i`, innermost first; locals read from `mem`
    /// (memory as it was then).
    pub fn call_stack(&self, i: usize, mem: &dyn Memory) -> Vec<Frame> {
        let t = self.tables;
        let calls = stack::calls_at(&self.rec.records, &self.rec.tasks, i);
        calls.into_iter().rev().map(|stack::Call { func, args, locals, .. }| {
            let f = t.functions.get(&func);
            let name = self.name(func);
            let args = args.map(|args| {
                let known = f.map_or(u32::MAX, |f| f.known);
                t.types.params.get(&name).into_iter().flatten().zip(args).enumerate().map(|(k, ((pname, ty), v))| {
                    let mut arg = t.types.object(&Bytes(&v.to_le_bytes()), pname, 0, *ty, 0);
                    if known >> k & 1 == 0 {
                        // (Not passed any more: the optimizer took it away.)
                        arg = Object { value: Some("(optimized out)".into()), children: None, expand: None, target: None, ..arg };
                    }
                    arg
                }).collect()
            });
            let locals = match (locals, t.locals.get(&func)) {
                (Some(addrs), Some(names)) => {
                    let types = t.types.locals.get(&name);
                    Some(addrs.into_iter().zip(names).filter_map(|(addr, var)| {
                        let &(_, ty) = types?.iter().find(|(n, _)| n == var)?;
                        Some(t.types.object(mem, var, addr, ty, 0))
                    }).collect())
                }
                _ => None,
            };
            Frame { func: name, file: f.map(|f| t.file(f.loc).to_string()), line: f.map(|f| f.loc.line), args, locals }
        }).collect()
    }

    /// The newest write to `addr..addr + len` at or before record `at`.
    pub fn last_write(&self, addr: u32, len: u32, at: usize) -> Option<Write> {
        let (i, old, line) = self.rec.last_write(self.tables, addr, len, at)?;
        let line = line.map(|s| self.line(&s));
        Some(Write { i, old, task: self.task(self.rec.tasks[i], i), func: line.as_ref().map(|l| l.func.clone()),
                     file: line.as_ref().map(|l| l.file.clone()), line: line.as_ref().map(|l| l.line),
                     inlined: line.and_then(|l| l.inlined) })
    }

    /// The task running at the start of each of `n` buckets of the recording.
    pub fn timeline(&self, n: usize) -> Vec<TaskRef> {
        let total = self.rec.len().max(1);
        (0..n).filter_map(|b| {
            let i = b * total / n;
            Some(self.task(*self.rec.tasks.get(i)?, i))
        }).collect()
    }

    /// The functions called `name` (usually one), with how often each block
    /// ran in syscall `n` (or the whole recording).
    pub fn functions(&self, name: &str, n: Option<usize>) -> Vec<Function> {
        let t = self.tables;
        let counts = self.rec.block_counts(n);
        t.functions.iter().filter(|(_, f)| f.name == name).map(|(&id, f)| Function {
            name: f.name.clone(),
            file: t.file(f.loc).to_string(),
            line: f.loc.line,
            blocks: t.blocks_of(id).map(|(n, l)| Block {
                n, file: t.file(l).to_string(), line: l.line, count: counts.get(&(id, n)).copied().unwrap_or(0),
            }).collect(),
        }).collect()
    }

    /// Every line of `file` where a block starts, with how often blocks
    /// there ran in syscall `n` (or the whole recording).
    pub fn coverage(&self, file: &str, n: Option<usize>) -> Vec<LineCount> {
        let t = self.tables;
        let counts = self.rec.block_counts(n);
        let mut lines: BTreeMap<u32, u64> = BTreeMap::new();
        for (&(f, b), l) in &t.blocks {
            if t.file(*l) == file {
                let c = lines.entry(l.line).or_default();
                *c = (*c).max(counts.get(&(f, b)).copied().unwrap_or(0));
            }
        }
        lines.into_iter().map(|(line, count)| LineCount { line, count }).collect()
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
            name: name.to_string(), ty: self.name(ty), addr, size: self.size_of(ty),
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
