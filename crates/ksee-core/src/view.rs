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
            Record::Locals { .. } => ("locals", 0, None),
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
            let (raw, locals) = stack::args_locals(records, tasks, c.func, c.first, i);
            let locals = locals.and_then(|a| self.locals(c.func, &name, a, mem));
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

    /// Function `func`'s (called `name`) locals in memory, at `addrs`.
    fn locals(&self, func: u32, name: &str, addrs: Vec<u32>, mem: &dyn Memory) -> Option<Vec<Object>> {
        let t = self.tables;
        let (names, types) = (t.locals.get(&func)?, t.types.locals.get(name));
        Some(addrs.into_iter().zip(names).filter_map(|(addr, var)| {
            let &(_, ty) = types?.iter().find(|(n, _)| n == var)?;
            Some(t.types.object(mem, var, addr, ty, 0))
        }).collect())
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
            name: name.to_string(), ty: self.name(ty), addr, size: self.size_of(ty),
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
