//! Views of a recording, for the page (serializable with the "serde"
//! feature): what it shows, by name rather than by id.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use alloc::collections::BTreeMap;

use crate::arch;
use crate::recording::Recording;
use crate::stack::{self, LineStep};
use crate::tables::{Loc, Tables};
use crate::task::read_task;
use crate::trace::Record;
use crate::types::{Kind, Types};
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
    /// Its first record's id (see Recording::ids): the same in later
    /// snapshots, where its index has moved on.
    pub id: u32,
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
    /// The function the code there is inlined from, if.
    pub inlined: Option<String>,
    pub task: TaskRef,
    /// The syscall in progress on the task (its index), if any.
    pub syscall: Option<usize>,
}

/// A source line the kernel ran (see stack::lines), at record `i`, the
/// `nth` of the lines it runs there (see call_stack).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Line {
    pub i: usize,
    pub nth: u32,
    pub func: String,
    pub file: String,
    pub line: u32,
    /// The function the line is inlined from, if.
    pub inlined: Option<String>,
    /// How deep it runs: calls deep, and inlined (how many functions its
    /// code is inlined from): a stepper's level.
    pub depth: u32,
    pub inl: u32,
}

/// A call on a stack, with its arguments (32 bits each: wider ones lose
/// their high half) and its locals in memory.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Frame {
    pub func: String,
    /// The call's first record (its block 0).
    pub first: usize,
    /// How many calls deep it is (as line steps count: see Line::depth).
    pub depth: u32,
    /// The syscall entry (with its wrappers, see arch::SYSCALL_WRAPPERS):
    /// the syscall's name; its arguments are the syscall's.
    pub syscall: Option<String>,
    /// Code inlined into the call (the frame outside it), not a call.
    pub inlined: bool,
    /// Where the call is (the line it runs, or the one calling the frame
    /// inside it; else where its function is).
    pub file: Option<String>,
    pub line: Option<u32>,
    pub args: Option<Vec<Object>>,
    pub locals: Option<Vec<Object>>,
}

/// A write to an object, in its history: at record `i`, to its `member`
/// (a path: "f_path.dentry"; "" the object itself), from `before` to
/// `after` (the member's value, printed), on `task`, by the code at `at`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Change {
    pub i: usize,
    pub member: String,
    pub before: String,
    pub after: String,
    pub task: TaskRef,
    pub at: Option<Line>,
}

/// An object written in a span of records: `what` it is ("struct file
/// 0x1234", "ksys_read's iocb", "current task_struct"), of type `ty` (id
/// `id`) at `addr`; `n` writes to it, the newest `writes` of them; `call`:
/// the call (its first record) it lives as long as, for a local or a frame.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Changed {
    pub what: String,
    pub ty: String,
    pub id: u32,
    pub addr: u32,
    pub n: usize,
    pub writes: Vec<Change>,
    pub call: Option<usize>,
}

/// The newest write to a value: at record `i`, on `task`, by the code at
/// `at` (the line, if known); `old` what it overwrote.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Write {
    pub i: usize,
    pub old: u64,
    pub task: TaskRef,
    pub at: Option<Line>,
}

/// A function: where it is, from `line` to `last` (of its blocks in its
/// own file).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Function {
    pub name: String,
    pub file: String,
    pub line: u32,
    pub last: u32,
    /// Only its call and returns are recorded (tables::Function::sealed).
    pub sealed: bool,
}

/// The calls of a function from one place, in a recording.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Caller {
    pub func: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub count: u32,
    /// The first such call's record.
    pub first: usize,
}

/// Where a function is.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Place {
    pub name: String,
    pub file: String,
    pub line: u32,
}

/// A syscall: its number, and its parameters (as SYSCALL_DEFINE declares
/// them).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct SyscallInfo {
    pub nr: u32,
    pub params: Vec<Param>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Param {
    pub name: String,
    pub ty: String,
    /// The starts of the constants' names that fit it ("O_", "AT_"), if any.
    pub consts: Vec<String>,
}

/// The constants that fit a syscall's parameter, by the starts of their
/// names: for "syscall.param", else any syscall's parameter of that name.
const CONSTS: &[(&str, &[&str])] = &[
    ("mmap2.flags", &["MAP_"]), ("openat.flags", &["O_"]), ("unlinkat.flag", &["AT_"]), ("renameat2.flags", &["RENAME_"]),
    ("waitid.which", &["P_"]), ("waitid.options", &["W"]), ("clone.clone_flags", &["CLONE_", "SIG"]),
    ("futex_time64.op", &["FUTEX_"]), ("clock_nanosleep_time64.which_clock", &["CLOCK_"]), ("clock_nanosleep_time64.flags", &["TIMER_"]),
    ("dfd", &["AT_"]), ("olddfd", &["AT_"]), ("newdfd", &["AT_"]), ("flags", &["O_"]), ("prot", &["PROT_"]),
    ("which_clock", &["CLOCK_"]), ("op", &["FUTEX_"]),
];

fn consts_of(syscall: &str, param: &str) -> &'static [&'static str] {
    let key = format!("{syscall}.{param}");
    CONSTS.iter().find(|(k, _)| *k == key).or_else(|| CONSTS.iter().find(|(k, _)| *k == param)).map_or(&[], |(_, c)| c)
}

/// Value `v` as the constants whose names start with one of `prefixes`
/// name it: one equal to it, or flags (one bit each) that make it up.
fn constant_name(constants: &BTreeMap<String, i64>, prefixes: &[&str], v: i64) -> Option<String> {
    let fits: Vec<(&String, i64)> = constants.iter().filter(|(n, _)| prefixes.iter().any(|p| n.starts_with(p))).map(|(n, &x)| (n, x)).collect();
    if let Some((n, _)) = fits.iter().find(|(_, x)| *x == v) {
        return Some((*n).clone());
    }
    if v <= 0 {
        return None;
    }
    let names: Vec<&str> = (0..32).filter(|b| v >> b & 1 != 0)
        .filter_map(|b| fits.iter().find(|(_, x)| *x == 1 << b).map(|(n, _)| n.as_str())).collect();
    (names.len() == v.count_ones() as usize).then(|| names.join(" | "))
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
    /// Its type's id (for Machine.object_by_id, Machine.writes).
    pub id: u32,
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

/// An argument's value, as if it were memory at address 0; past it, memory
/// (what a pointer argument points to).
struct Arg<'a>(&'a [u8], &'a dyn Memory);

impl Memory for Arg<'_> {
    fn read(&self, addr: u32, buf: &mut [u8]) {
        if addr as usize >= self.0.len() {
            return self.1.read(addr, buf);
        }
        for (i, b) in buf.iter_mut().enumerate() {
            *b = self.0.get(addr as usize + i).copied().unwrap_or(0);
        }
    }
}

impl View<'_> {
    fn name(&self, func: u32) -> String {
        self.tables.functions.get(&func).map_or_else(|| format!("{func:#x}"), |f| f.name.clone())
    }

    /// Who the task at `addr` is (as memory says now).
    pub fn task(&self, addr: u32) -> TaskRef {
        let t = read_task(self.tables, self.now, addr);
        TaskRef { addr, pid: t.as_ref().map(|t| t.pid), comm: t.map(|t| t.comm) }
    }

    pub fn syscalls(&self) -> Vec<SyscallSummary> {
        self.rec.syscalls.iter().enumerate().map(|(index, s)| SyscallSummary {
            index,
            id: self.rec.ids.get(s.first).copied().unwrap_or(0),
            task: self.task(s.task),
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
        let mut inlined = None;
        let (kind, func, loc) = match *self.rec.records.get(i)? {
            Record::Block { func, n } => {
                inlined = t.segments(func, n).first().and_then(|s| s.first()).and_then(|s| s.inlined);
                ("block", func, t.blocks.get(&(func, n)).copied())
            }
            Record::Return { func, .. } => ("return", func, t.functions.get(&func).map(|f| f.loc)),
            Record::Args { .. } => ("args", 0, None),
            Record::Store { .. } => ("store", 0, None),
        };
        Some(At {
            i, kind,
            func: t.functions.get(&func).map_or(String::new(), |f| f.name.clone()),
            file: loc.map(|l| t.file(l).to_string()),
            line: loc.map(|l| l.line),
            inlined: self.inlined_from(inlined),
            task: self.task(self.rec.tasks[i]),
            syscall: self.rec.syscall_at(i),
        })
    }

    /// Records `from..=to` as text, a line each, as the recording has them
    /// (the raw trace): "block ksys_read #3 fs/read_write.c:710", "args fd=3
    /// buf=0x1cefed0 count=64", "locals iocb@0x1ce000", "store 0x1ce0020
    /// [8] 0 → 26", "return ksys_read = 26"; a task switch as a line of
    /// its own.
    pub fn raw(&self, from: usize, to: usize) -> Vec<(usize, String)> {
        let (t, rec) = (self.tables, self.rec);
        let mut out = Vec::new();
        let mut func_before = None;
        for i in from..=to.min(records_len(rec)) {
            if i > 0 && rec.tasks.get(i) != rec.tasks.get(i - 1) {
                let task = self.task(rec.tasks[i]);
                out.push((i, format!("── {}({}) runs", task.comm.unwrap_or_default(), task.pid.unwrap_or(0))));
            }
            let text = match rec.records[i] {
                Record::Block { func, n } => {
                    func_before = Some(func);
                    let loc = t.blocks.get(&(func, n)).map_or(String::new(), |l| format!(" {}:{}", t.file(*l), l.line));
                    format!("block {} #{n}{loc}", self.name(func))
                }
                Record::Return { func, value } => format!("return {} = {}", self.name(func), value as i32),
                Record::Args { args, n, frame } => {
                    let params = func_before.and_then(|f| t.functions.get(&f)).and_then(|f| t.types.params.get(&f.name));
                    let each = args[..n as usize].iter().enumerate().map(|(k, &v)| {
                        let name = params.and_then(|p| p.get(k)).map_or(format!("{k}"), |p| p.0.clone());
                        format!("{name}={}", if v > 0xffff { format!("{v:#x}") } else { v.to_string() })
                    });
                    format!("args {} frame={frame:#x}", each.collect::<Vec<_>>().join(" "))
                }
                Record::Store { addr, size: 0, old } => format!("store {addr:#x} [{old} bytes] (old contents not kept)"),
                Record::Store { addr, size, old } => {
                    let mut new = [0u8; 8];
                    rec.memory_at(self.now, i).read(addr, &mut new[..size.min(8) as usize]);
                    let new = new.iter().rev().fold(0u64, |v, &x| v << 8 | x as u64);
                    let num = |v: u64| if v > 0xffff { format!("{v:#x}") } else { v.to_string() };
                    format!("store {addr:#x} [{size}] {} → {}", num(old), num(new))
                }
            };
            out.push((i, text));
        }
        out
    }

    fn line(&self, s: &LineStep) -> Line {
        let t = self.tables;
        Line { i: s.record, nth: s.nth, func: self.name(s.func), file: t.file(s.loc).to_string(), line: s.loc.line,
               inlined: self.inlined_from(s.inlined), depth: s.depth, inl: self.chain(s.inlined).len() as u32 }
    }

    /// The source lines run in records `first..=last` of the task at `task`.
    pub fn lines(&self, first: usize, last: usize, task: u32) -> Vec<Line> {
        self.rec.lines(self.tables, first, last, task).iter().map(|s| self.line(s)).collect()
    }

    /// The call stack at record `i`, innermost first; locals read from `mem`
    /// (memory as it was then). A call is where its last line is: at `i`,
    /// the `nth` line that record runs (see lines), or its last. Code
    /// inlined there is a frame of its own for each function it is inlined
    /// from (`inlined`, with no arguments: there was no call).
    pub fn call_stack(&self, i: usize, nth: Option<usize>, mem: &dyn Memory) -> Vec<Frame> {
        let t = self.tables;
        let (records, tasks) = (&self.rec.records, &self.rec.tasks);
        let mut out = Vec::new();
        let entry = t.find(arch::SYSCALL);
        for (depth, c) in self.fold(stack::stack_at(t, records, tasks, i, nth)).into_iter().rev() {
            let name = self.name(c.func);
            let (raw, frame) = match stack::args_frame(records, c.first) {
                Some((a, f)) => (Some(a), Some(f)),
                None => (None, None),
            };
            let locals = frame.map(|f| self.locals(&name, f, mem));
            // (The syscall entry: its arguments are the syscall's.)
            let at_entry = matches!(records.get(c.first), Some(&Record::Block { func, n: 0 }) if Some(func) == entry);
            let (syscall, args) = match raw {
                Some(raw) => match at_entry.then(|| self.syscall_args(&raw, mem)).flatten() {
                    Some((n, a)) => (Some(n), Some(a)),
                    None => (None, Some(self.args(c.func, &name, raw, mem))),
                },
                None => (None, None),
            };
            // (The code inlined into it first, innermost first; then it.)
            let frame = |func, inlined, at: Option<Loc>| Frame {
                func, first: c.first, depth, syscall: None, inlined, file: at.map(|l| t.file(l).to_string()), line: at.map(|l| l.line), args: None, locals: None,
            };
            let nest = self.nest(&c);
            let (&(_, at), inner) = nest.split_last().expect("a call");
            out.extend(inner.iter().map(|&(k, at)| frame(self.inline_name(k.unwrap_or(0)), true, at)));
            out.push(Frame { args, locals, syscall, ..frame(name, false, at) });
        }
        out
    }

    /// A call stack (outermost first) with the syscall wrappers folded into
    /// the entry (arch::SYSCALL_WRAPPERS): one call, from the entry's first
    /// record, in the innermost wrapper (its function, at its line); each
    /// call with how deep it is.
    pub fn fold(&self, calls: Vec<stack::ChartCall>) -> Vec<(u32, stack::ChartCall)> {
        let entry = self.tables.find(arch::SYSCALL);
        let mut out: Vec<(u32, stack::ChartCall)> = Vec::new();
        let mut in_entry = false;
        for (d, c) in calls.into_iter().enumerate() {
            match out.last_mut() {
                Some((_, e)) if in_entry && self.is_wrapper(c.func) => *e = stack::ChartCall { first: e.first, ..c },
                _ => {
                    in_entry = Some(c.func) == entry;
                    out.push((d as u32, c));
                }
            }
        }
        out
    }

    fn is_wrapper(&self, func: u32) -> bool {
        self.tables.functions.get(&func).is_some_and(|f| arch::SYSCALL_WRAPPERS.iter().any(|w| f.name.starts_with(w)))
    }

    /// The syscall entry's arguments (the number, then the syscall's): the
    /// syscall's name, and its arguments as its parameters have them (with
    /// the constants that name them).
    fn syscall_args(&self, entry: &[u32], mem: &dyn Memory) -> Option<(String, Vec<Object>)> {
        let types = &self.tables.types;
        let name = types.syscalls.get(entry.first()?)?;
        let params = types.syscall_params(name)?;
        let args = params.iter().zip(entry.iter().skip(1).copied()).map(|((pname, ty), v)| {
            let mut o = types.object(&Arg(&v.to_le_bytes(), mem), pname, 0, *ty, 0);
            let names = constant_name(&types.constants, consts_of(name, pname), v as i32 as i64);
            o.value = names.or(o.value);
            o
        }).collect();
        Some((name.clone(), args))
    }

    /// Function `func`'s (called `name`) arguments, their values `args`: the
    /// source's parameters, as values of their types.
    fn args(&self, func: u32, name: &str, args: Vec<u32>, mem: &dyn Memory) -> Vec<Object> {
        let t = self.tables;
        let known = t.functions.get(&func).map_or(u32::MAX, |f| f.known);
        t.types.params.get(name).into_iter().flatten().zip(args).enumerate().map(|(k, ((pname, ty), v))| {
            let arg = t.types.object(&Arg(&v.to_le_bytes(), mem), pname, 0, *ty, 0);
            // (Not passed any more: the optimizer took it away.)
            if known >> k & 1 == 0 {
                return Object { value: Some("(optimized out)".into()), children: None, expand: None, target: None, ..arg };
            }
            arg
        }).collect()
    }

    /// Function `name`'s locals in memory, at their offsets from `frame`.
    fn locals(&self, name: &str, frame: u32, mem: &dyn Memory) -> Vec<Object> {
        let types = &self.tables.types;
        types.locals.get(name).into_iter().flatten()
            .map(|(var, ty, off)| types.object(mem, var, frame.wrapping_add(*off), *ty, 0)).collect()
    }

    /// Call `c` and the code inlined into it where it is, innermost first:
    /// each inlined function (Some: its index, see inline_name) at its line
    /// there, then the call (None) at its own line (where the outermost
    /// inlined code is inlined, or the line it is at).
    pub fn nest(&self, c: &stack::ChartCall) -> Vec<(Option<u32>, Option<Loc>)> {
        let mut at = c.step.map(|s| s.loc).or(self.tables.functions.get(&c.func).map(|f| f.loc));
        let mut out = Vec::new();
        for &(k, site) in self.chain(c.step.and_then(|s| s.inlined)) {
            // (A syscall's body, inlined into its wrapper: the entry's own.)
            if arch::SYSCALL_WRAPPERS.iter().any(|w| self.inline_name(k).starts_with(w)) {
                continue;
            }
            out.push((Some(k), at));
            at = Some(site);
        }
        out.push((None, at));
        out
    }

    /// Inline chain `k` (Tables::inlines), or none.
    pub fn chain(&self, k: Option<u32>) -> &[(u32, Loc)] {
        k.and_then(|k| self.tables.inlines.get(k as usize)).map_or(&[], Vec::as_slice)
    }

    /// The function code of inline chain `k` is inlined from (the innermost),
    /// if it is inlined.
    fn inlined_from(&self, k: Option<u32>) -> Option<String> {
        self.chain(k).first().map(|&(f, _)| self.inline_name(f))
    }

    /// Inlined function `k`'s name (Tables::names).
    pub fn inline_name(&self, k: u32) -> String {
        self.tables.names.get(k as usize).cloned().unwrap_or_default()
    }

    /// The newest write to `addr..addr + len` at or before record `at`.
    pub fn last_write(&self, addr: u32, len: u32, at: usize) -> Option<Write> {
        let (i, old, line) = self.rec.last_write(self.tables, addr, len, at)?;
        Some(Write { i, old, task: self.task(self.rec.tasks[i]), at: line.map(|s| self.line(&s)) })
    }

    /// An object's history: the newest `limit` stores to the value of type
    /// `ty` at `addr` in records `span` (its lifetime), oldest first, each
    /// with the member it wrote (the one its first byte is in: see
    /// Types::leaf), as it was before and after.
    pub fn writes(&self, addr: u32, ty: u32, span: core::ops::RangeInclusive<usize>, limit: usize) -> Vec<Change> {
        let all = self.rec.writes(addr, self.tables.types.size_of(ty).max(1), span);
        all[all.len().saturating_sub(limit)..].iter().map(|&i| self.change(addr, ty, i)).collect()
    }

    /// What the object of type `ty` at `addr` is, said as the kernel would,
    /// from its own fields in `mem`: a task "init(1)", a file "fd 3
    /// /note.txt" (its fd in `task`'s table), an inode "inode 12 (ext4)", a
    /// dentry, a path or a kiocb by their name, a super_block by its type
    /// and id. None for other types.
    pub fn describe(&self, mem: &dyn Memory, ty: u32, addr: u32, task: u32) -> Option<String> {
        let types = &self.tables.types;
        let ty = types.resolve(ty);
        let name = types.name(ty);
        let kind = name.strip_prefix("struct ")?;
        // (Field `f` of the `s` at `p`: its address; `ptr`: the pointer there.)
        let at = |p: u32, s: &str, f: &str| Some(p.checked_add(types.field(s, f)?.0)?);
        let ptr = |p: u32, s: &str, f: &str| Some(mem.read_u32(at(p, s, f)?)).filter(|&q| q != 0);
        let dentry = |d: u32| c_string(mem, ptr(at(d, "dentry", "d_name")?, "qstr", "name")?).map(|s| s.trim_matches('"').to_string());
        let fs_name = |sb: u32| c_string(mem, ptr(ptr(sb, "super_block", "s_type")?, "file_system_type", "name")?);
        match kind {
            "task_struct" => {
                let t = read_task(self.tables, mem, addr)?;
                Some(format!("{}({})", t.comm, t.pid))
            }
            "dentry" => Some(format!("dentry {}", dentry(addr)?)),
            "path" => Some(format!("path {}", dentry(ptr(addr, "path", "dentry")?)?)),
            "inode" => Some(format!("inode {}{}", mem.read_u32(at(addr, "inode", "i_ino")?),
                                    ptr(addr, "inode", "i_sb").and_then(fs_name).map_or(String::new(), |n| format!(" ({})", n.trim_matches('"'))))),
            "super_block" => Some(format!("super_block {} {}", fs_name(addr)?.trim_matches('"'), c_string(mem, at(addr, "super_block", "s_id")?)?)),
            "file" | "kiocb" => {
                let file = if kind == "kiocb" { ptr(addr, "kiocb", "ki_filp")? } else { addr };
                let name = dentry(ptr(at(file, "file", "f_path")?, "path", "dentry")?)?;
                // (Its fd: the one in the task's table pointing at it.)
                let fd = (|| {
                    let fdt = ptr(ptr(task, "task_struct", "files")?, "files_struct", "fdt")?;
                    let (fds, max) = (ptr(fdt, "fdtable", "fd")?, mem.read_u32(at(fdt, "fdtable", "max_fds")?).min(256));
                    (0..max).find(|k| mem.read_u32(fds + 4 * k) == file)
                })();
                let what = match fd { Some(k) => format!("fd {k} {name}"), None => format!("file {name}") };
                Some(if kind == "kiocb" { format!("kiocb on {what}") } else { what })
            }
            _ => None,
        }
    }

    /// Store `i` as a change to the object of type `ty` at `addr`.
    fn change(&self, addr: u32, ty: u32, i: usize) -> Change {
        let types = &self.tables.types;
        let Record::Store { addr: a, size, old } = self.rec.records[i] else { unreachable!() };
        let task = self.task(self.rec.tasks[i]);
        let at = self.rec.line_at(self.tables, i).map(|s| self.line(&s));
        // (No type, a frame's bytes: the store as it is, at its offset.)
        if ty == 0 {
            let num = |v: u64| if v > 0xffff { format!("{v:#x}") } else { v.to_string() };
            let mut new = [0u8; 8];
            self.rec.memory_at(self.now, i).read(a, &mut new[..size.min(8) as usize]);
            let new = new.iter().rev().fold(0u64, |v, &x| v << 8 | x as u64);
            return Change { i, member: format!("+{}", a.wrapping_sub(addr)), before: num(old), after: num(new), task, at };
        }
        let (member, leaf, off, bits) = types.leaf(ty, a.max(addr) - addr);
        let (b0, bn) = bits.unwrap_or((0, 0));
        let value = |at: usize| {
            let mut v = String::new();
            types.print(&self.rec.memory_at(self.now, at), addr + off, leaf, b0, bn, &mut v);
            v
        };
        Change { i, member, before: value(i.saturating_sub(1)), after: value(i), task, at }
    }

    /// What records `first..=last` (a call's) changed: the objects written,
    /// in the order of their first write, each with its writes (the newest
    /// `limit`). An object is known by what the code there held: a call's
    /// pointer argument to a struct, a local, the task running, a callee's
    /// frame (from its frame base up to its caller's: the stack it used
    /// beyond its named locals; no type) (the rest of the stores, to memory
    /// nothing names, are counted in the last entry, "other").
    pub fn changes(&self, first: usize, last: usize, limit: usize) -> Vec<Changed> {
        let (t, types) = (self.tables, &self.tables.types);
        // (The objects: what, type, address, size. The task first. Each
        // said as what it is, as memory at the span's end says: see describe.)
        let task = self.rec.tasks.get(first).copied().unwrap_or(0);
        let end = self.rec.memory_at(self.now, last.min(records_len(self.rec)));
        let mut regions: Vec<(String, u32, u32, u32, Option<usize>)> = Vec::new();
        let mut seen = alloc::collections::BTreeSet::new();
        let mut add = |what: String, ty: u32, addr: u32, size: Option<u32>, call: Option<usize>| {
            let ty = types.resolve(ty);
            if addr != 0 && seen.insert((addr, ty)) {
                let what = self.describe(&end, ty, addr, task).unwrap_or(what);
                regions.push((what, ty, addr, size.unwrap_or_else(|| types.size_of(ty)), call));
            }
        };
        if let Some(ty) = types.find("task_struct") {
            add("current task_struct".into(), ty, task, None, None);
        }
        let records = &self.rec.records;
        // (The calls open on each task: (function, frame base).)
        let mut open: alloc::collections::BTreeMap<u32, Vec<(u32, u32)>> = alloc::collections::BTreeMap::new();
        for i in first..=last.min(records.len().saturating_sub(1)) {
            match records[i] {
                Record::Return { func, .. } => {
                    if let Some(stack) = open.get_mut(&self.rec.tasks[i]) && stack.last().is_some_and(|c| c.0 == func) {
                        stack.pop();
                    }
                }
                Record::Block { func, n: 0 } => {
                    let Some((f, &Record::Args { args, frame, .. })) = t.functions.get(&func).zip(records.get(i + 1)) else { continue };
                    // (Its frame: up to its caller's, if the caller is known.)
                    let stack = open.entry(self.rec.tasks[i]).or_default();
                    if let Some(&(_, caller)) = stack.last() && caller > frame {
                        add(format!("{}'s frame", f.name), 0, frame, Some(caller - frame), Some(i));
                    }
                    stack.push((func, frame));
                    for ((pname, ty), &v) in types.params.get(&f.name).into_iter().flatten().zip(&args) {
                        if let Kind::Pointer(to) = types.get(types.resolve(*ty)).kind
                            && matches!(types.get(types.resolve(to)).kind, Kind::Struct(_) | Kind::Union(_)) {
                            add(format!("{} {v:#x} ({}'s {pname})", types.name(types.resolve(to)), f.name), to, v, None, None);
                        }
                    }
                    for (var, ty, off) in types.locals.get(&f.name).into_iter().flatten() {
                        add(format!("{}'s {var}", f.name), *ty, frame.wrapping_add(*off), None, Some(i));
                    }
                }
                _ => {}
            }
        }
        // (Each store to the smallest object holding its first byte.)
        let mut hits: Vec<Vec<usize>> = alloc::vec![Vec::new(); regions.len()];
        let mut other = 0;
        for &i in self.rec.stores_between(first, last) {
            let Record::Store { addr, .. } = records[i] else { continue };
            match regions.iter().enumerate().filter(|(_, r)| r.2 <= addr && addr < r.2.saturating_add(r.3))
                    .min_by_key(|(_, r)| r.3) {
                Some((k, _)) => hits[k].push(i),
                None => other += 1,
            }
        }
        let mut out: Vec<Changed> = regions.iter().zip(hits).filter(|(_, h)| !h.is_empty()).map(|((what, ty, addr, _, call), h)| Changed {
            what: what.clone(), ty: types.name(*ty), id: *ty, addr: *addr, n: h.len(),
            writes: h[h.len().saturating_sub(limit)..].iter().map(|&i| self.change(*addr, *ty, i)).collect(), call: *call,
        }).collect();
        out.sort_by_key(|c| c.writes[0].i);
        if other > 0 {
            out.push(Changed { what: "other".into(), ty: String::new(), id: 0, addr: 0, n: other, writes: Vec::new(), call: None });
        }
        out
    }

    /// The task running at the start of each of `n` buckets of the recording.
    pub fn timeline(&self, n: usize) -> Vec<TaskRef> {
        let total = self.rec.len().max(1);
        (0..n).filter_map(|b| {
            let i = b * total / n;
            Some(self.task(*self.rec.tasks.get(i)?))
        }).collect()
    }

    /// Who called function `name` in syscall `n` (or the whole recording):
    /// each caller at each line, by how often.
    pub fn callers(&self, name: &str, n: Option<usize>) -> Vec<Caller> {
        let t = self.tables;
        let ids: Vec<u32> = t.functions.iter().filter(|(_, f)| f.name == name).map(|(&id, _)| id).collect();
        let Some((first, last, task)) = self.rec.span(n) else { return Vec::new() };
        let entry = t.find(arch::SYSCALL);
        stack::callers(t, &self.rec.records, &self.rec.tasks, first, last, task, &ids).into_iter().map(|c| Caller {
            // (The syscall entry, or its wrappers: named for the syscall, as the Stack has it.)
            func: match self.rec.syscall_at(c.first).and_then(|n| t.types.syscalls.get(&self.rec.syscalls[n].nr)) {
                Some(s) if Some(c.func) == entry || self.is_wrapper(c.func) => format!("{s}()"),
                _ => self.name(c.func),
            },
            file: c.step.map(|s| t.file(s.loc).to_string()), line: c.step.map(|s| s.loc.line),
            count: c.count, first: c.first,
        }).collect()
    }

    /// Where `name` is defined: an instrumented function (its source), else
    /// a function with no probe (inline), a type, a global, or a member as
    /// "struct.member" (".member": of any struct) (where it is declared).
    pub fn definition(&self, name: &str) -> Option<Place> {
        if let Some(id) = self.tables.find(name) {
            return self.place(id);
        }
        let declared = &self.tables.types.declared;
        // (".member": a member of any struct, when whose is not known.)
        let (file, line) = declared.get(name).or_else(|| declared.iter().find(|(k, _)| k.ends_with(name) && name.starts_with('.')).map(|(_, v)| v))?;
        Some(Place { name: name.to_string(), file: file.clone(), line: *line })
    }

    /// Function `id`: where it is.
    pub fn place(&self, id: u32) -> Option<Place> {
        let f = self.tables.functions.get(&id)?;
        Some(Place { name: f.name.clone(), file: self.tables.file(f.loc).to_string(), line: f.loc.line })
    }

    /// Syscall `name` (e.g. "openat").
    pub fn syscall(&self, name: &str) -> Option<SyscallInfo> {
        let types = &self.tables.types;
        let (&nr, _) = types.syscalls.iter().find(|(_, n)| n.as_str() == name)?;
        let params = types.syscall_params(name).into_iter().flatten()
            .map(|(pname, ty)| Param { name: pname.clone(), ty: types.name(*ty), consts: consts_of(name, pname).iter().map(|s| s.to_string()).collect() }).collect();
        Some(SyscallInfo { nr, params })
    }

    /// The functions called `name` (usually one).
    pub fn functions(&self, name: &str) -> Vec<Function> {
        let t = self.tables;
        t.functions.iter().filter(|(_, f)| f.name == name).map(|(&id, f)| Function {
            name: f.name.clone(),
            file: t.file(f.loc).to_string(),
            line: f.loc.line,
            last: t.blocks_of(id).filter(|(_, l)| l.file == f.loc.file).map(|(_, l)| l.line).fold(f.loc.line, u32::max),
            sealed: f.sealed,
        }).collect()
    }

    /// The lines of `file` that code ran, in syscall `n` (or the whole
    /// recording), with how often (the most of the blocks running it).
    /// Lines inlined into a block are its too.
    pub fn coverage(&self, file: &str, n: Option<usize>) -> Vec<LineCount> {
        let t = self.tables;
        let counts = self.rec.block_counts(n);
        let files: Vec<u32> = (0..t.files.len() as u32).filter(|&k| t.files[k as usize] == file).collect();
        let mut lines: BTreeMap<u32, u64> = BTreeMap::new();
        for (&(f, b), &c) in counts.iter().filter(|(_, c)| **c > 0) {
            let Some(&l) = t.blocks.get(&(f, b)) else { continue };
            let mut mark = |loc: Loc| {
                if files.contains(&loc.file) {
                    let e = lines.entry(loc.line).or_default();
                    *e = (*e).max(c);
                }
            };
            match t.lines.get(&(f, b)) {
                Some(segs) => segs.iter().flatten().for_each(|s| mark(s.loc)),
                None => mark(l),
            }
        }
        lines.into_iter().map(|(line, count)| LineCount { line, count }).collect()
    }
}

impl Types {
    /// The value of type `ty` at `addr`, aggregates expanded `depth` deep.
    pub fn object(&self, mem: &dyn Memory, name: &str, addr: u32, ty: u32, depth: u32) -> Object {
        self.object_bits(mem, name, addr, ty, None, depth)
    }

    /// The same, for a value that may be a bitfield: `bits`, its (offset in
    /// the byte at `addr`, size).
    fn object_bits(&self, mem: &dyn Memory, name: &str, addr: u32, ty: u32, bits: Option<(u32, u32)>, depth: u32) -> Object {
        let id = self.resolve(ty);
        let t = self.get(id);
        let mut o = Object {
            name: name.to_string(), ty: self.name(ty), addr, id: ty, size: self.size_of(ty),
            value: None, children: None, expand: None, target: None,
        };
        match &t.kind {
            Kind::Struct(_) | Kind::Union(_) => {
                if depth == 0 {
                    o.expand = Some(id);
                } else {
                    o.children = Some(self.members(&t.kind).iter().map(|m| {
                        let bits = (m.bit_size > 0).then_some((m.bit_offset % 8, m.bit_size));
                        self.object_bits(mem, &m.name, addr + m.bit_offset / 8, m.ty, bits, depth - 1)
                    }).collect());
                }
            }
            Kind::Array { elem, count } if !self.is_string(*elem) => {
                if depth == 0 {
                    o.expand = Some(id);
                } else {
                    let esize = self.size_of(*elem);
                    o.children = Some((0..(*count).min(64)).map(|i| {
                        self.object_bits(mem, &format!("[{i}]"), addr + i * esize, *elem, None, depth - 1)
                    }).collect());
                }
            }
            Kind::Pointer(to) => {
                let p = mem.read_u32(addr);
                // (A char *: and the string it points to, if it looks like one.)
                let text = if p != 0 && self.is_string(*to) { c_string(mem, p) } else { None };
                o.value = Some(match text {
                    _ if p == 0 => "NULL".into(),
                    Some(t) => format!("{p:#x} {t}"),
                    None => format!("{p:#x}"),
                });
                let target = self.resolve(*to);
                if p != 0 && matches!(self.get(target).kind, Kind::Struct(_) | Kind::Union(_)) {
                    o.target = Some(target);
                }
            }
            _ => {
                let mut v = String::new();
                let (off, size) = bits.unwrap_or((0, 0));
                self.print(mem, addr, ty, off, size, &mut v);
                o.value = Some(v);
            }
        }
        o
    }
}

fn records_len(rec: &Recording) -> usize {
    rec.records.len().saturating_sub(1)
}

/// The C string at `p`, quoted (up to 64 characters, then "…"), if what is
/// there looks like one: printable, with at least one character. (A
/// __user one too: a program's memory is kernel memory.)
fn c_string(mem: &dyn Memory, p: u32) -> Option<String> {
    let mut buf = [0u8; 65];
    mem.read(p, &mut buf);
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let text = &buf[..n.min(64)];
    if text.is_empty() || !text.iter().all(|&c| (0x20..0x7f).contains(&c) || c == b'\n' || c == b'\t') {
        return None;
    }
    let s = format!("{:?}", String::from_utf8_lossy(text));
    Some(if n > 64 { s + "…" } else { s })
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
    fn an_object_has_a_history() {
        // struct s { int a; int b; } at 4: record 1 stores b (2, was 1),
        // record 3 a byte of a (5); in the span from record 2, the second only.
        let t = Tables::parse("", "T\t1\tbase\tint\t4\ts\nT\t2\tstruct\ts\t8\nM\t2\ta\t0\t0\t1\nM\t2\tb\t32\t0\t1\n");
        let r = [Record::Block { func: 9, n: 1 }, Record::Store { addr: 8, size: 4, old: 1 },
                 Record::Block { func: 9, n: 1 }, Record::Store { addr: 5, size: 1, old: 0 }];
        let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 4]);
        let mut m = alloc::vec![0u8; 16];
        (m[8], m[5]) = (2, 5);
        let v = View { tables: &t, rec: &rec, now: &Mem(m) };
        let got: Vec<_> = v.writes(4, 2, 0..=3, 8).into_iter().map(|c| (c.i, c.member, c.before, c.after)).collect();
        assert_eq!(got, [(1, "b".into(), "1".into(), "2".into()), (3, "a".into(), "0".into(), "1280".into())]);
        assert_eq!(v.writes(4, 2, 2..=3, 8).len(), 1);
    }

    #[test]
    fn objects_are_said_as_what_they_are() {
        // struct task { int pid; char comm[8] } at 32: init(1). struct qstr { char *name } at 16, its
        // name at 24 ("a.txt"); struct dentry { struct qstr d_name } at 16.
        let t = Tables::parse("", "T\t1\tbase\tint\t4\ts\nT\t2\tbase\tchar\t1\tc\nT\t3\tarray\t\t8\t2\t8\n\
                               T\t4\tstruct\ttask_struct\t12\nM\t4\tpid\t0\t0\t1\nM\t4\tcomm\t32\t0\t3\n\
                               T\t5\tptr\t\t4\t2\nT\t6\tstruct\tqstr\t4\nM\t6\tname\t0\t0\t5\n\
                               T\t7\tstruct\tdentry\t4\nM\t7\td_name\t0\t0\t6\n");
        let mut m = alloc::vec![0u8; 128];
        m[32] = 1;
        m[36..40].copy_from_slice(b"init");
        m[16..20].copy_from_slice(&24u32.to_le_bytes());
        m[24..29].copy_from_slice(b"a.txt");
        let rec = Recording::new(&t, Vec::new(), Vec::new());
        let v = View { tables: &t, rec: &rec, now: &Mem(m) };
        assert_eq!(v.describe(v.now, 4, 32, 0).as_deref(), Some("init(1)"));
        assert_eq!(v.describe(v.now, 7, 16, 0).as_deref(), Some("dentry a.txt"));
        assert_eq!(v.describe(v.now, 1, 0, 0), None);
    }

    #[test]
    fn a_call_changes_objects() {
        // f (1, "f") takes struct s *p (at 4) and has a local int x (at
        // frame 32 + 4): it stores p->b, then x; calls g (2), whose frame is
        // at 16, which stores a byte of its own frame; and one byte nobody
        // names.
        let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\nF\t2\tg\t0:9\t0\n",
                              "T\t1\tbase\tint\t4\ts\nT\t2\tstruct\ts\t8\nM\t2\ta\t0\t0\t1\nM\t2\tb\t32\t0\t1\nT\t3\tptr\t\t4\t2\n\
                               P\tf\tp:3\nQ\tf\tx:1:4\n");
        let r = [Record::Block { func: 1, n: 0 }, Record::Args { args: [4, 0, 0, 0, 0, 0], n: 1, frame: 32 },
                 Record::Store { addr: 8, size: 4, old: 1 }, Record::Store { addr: 36, size: 4, old: 0 },
                 Record::Block { func: 2, n: 0 }, Record::Args { args: [0; 6], n: 0, frame: 16 },
                 Record::Store { addr: 20, size: 1, old: 0 }, Record::Return { func: 2, value: 0 },
                 Record::Store { addr: 100, size: 1, old: 0 }];
        let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 9]);
        let mut m = alloc::vec![0u8; 128];
        (m[8], m[36], m[20]) = (2, 7, 9);
        let v = View { tables: &t, rec: &rec, now: &Mem(m) };
        let got: Vec<_> = v.changes(0, 8, 8).into_iter().map(|c| (c.what, c.n, c.writes.first().map(|w| (w.member.clone(), w.after.clone())))).collect();
        assert_eq!(got, [("struct s 0x4 (f's p)".into(), 1, Some(("b".into(), "2".into()))),
                         ("f's x".into(), 1, Some(("".into(), "7".into()))),
                         ("g's frame".into(), 1, Some(("+4".into(), "9".into()))), ("other".into(), 1, None)]);
    }

    #[test]
    fn frames_are_where_their_calls_are() {
        // f (1) runs lines 1 and 2, calls g (2) at 2, then runs 3.
        let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\nF\t2\tg\t0:9\t0\nB\t1\t0\t0:1:1\n\
                               L\t1\t0\t0:1;:2;|;:3\nB\t2\t0\t0:10:1\n", "");
        let r = [Record::Block { func: 1, n: 0 }, Record::Block { func: 2, n: 0 }];
        let rec = Recording::new(&t, r.to_vec(), alloc::vec![0, 1]);
        let v = View { tables: &t, rec: &rec, now: &Mem(Vec::new()) };
        let lines: Vec<_> = v.call_stack(1, None, &Mem(Vec::new())).iter().map(|f| (f.func.clone(), f.line)).collect();
        assert_eq!(lines, [("g".into(), Some(10)), ("f".into(), Some(2))]);
    }

    #[test]
    fn inlined_code_is_a_frame_of_its_own() {
        // f (1) runs line 1, then h's line 20 (h inlined into f at line 2),
        // which calls g (2).
        let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\nF\t2\tg\t0:9\t0\nB\t1\t0\t0:1:1\n\
                               L\t1\t0\t0:1;:20@h^0:2;|\nB\t2\t0\t0:10:1\n", "");
        let r = [Record::Block { func: 1, n: 0 }, Record::Block { func: 2, n: 0 }];
        let rec = Recording::new(&t, r.to_vec(), alloc::vec![0, 1]);
        let v = View { tables: &t, rec: &rec, now: &Mem(Vec::new()) };
        let frames: Vec<_> = v.call_stack(1, None, &Mem(Vec::new())).iter().map(|f| (f.func.clone(), f.line, f.inlined)).collect();
        assert_eq!(frames, [("g".into(), Some(10), false), ("h".into(), Some(20), true), ("f".into(), Some(2), false)]);
        // At record 0, its first line (nth 0): f at line 1, nothing inlined.
        let first: Vec<_> = v.call_stack(0, Some(0), &Mem(Vec::new())).iter().map(|f| (f.func.clone(), f.line)).collect();
        assert_eq!(first, [("f".into(), Some(1))]);
    }

    #[test]
    fn char_pointers_show_their_strings() {
        let t = Types::parse("T\t1\tbase\tchar\t1\tc\nT\t2\tptr\t\t4\t1\n");
        let mut m = alloc::vec![0u8; 128];
        m[0..4].copy_from_slice(&8u32.to_le_bytes());
        m[8..12].copy_from_slice(b"tmp\0");
        m[4..8].copy_from_slice(&16u32.to_le_bytes()); // (to bytes that are no string)
        m[16] = 0xff;
        let value = |at| t.object(&Mem(m.clone()), "p", at, 2, 0).value.unwrap();
        assert_eq!((value(0).as_str(), value(4).as_str()), ("0x8 \"tmp\"", "0x10"));
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
