//! Views of a recording, for the page (serializable with the "serde"
//! feature): what it shows, by name rather than by id.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use alloc::collections::BTreeMap;

use crate::arch;
use crate::recording::Recording;
use crate::stack::{self, LineStep};
use crate::tables::{Loc, Step, Tables};
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
    /// A return's value (truncated to 32 bits).
    pub value: Option<i32>,
}

/// A source line the kernel ran (see stack::lines), at record `i`, the
/// `nth` of the lines it runs there (see call_stack).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Line {
    pub i: usize,
    /// The record the line is at: its first store's, else `i` (LineStep::at).
    pub at: usize,
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
    /// Which turn of a loop it is (see LineStep::run).
    pub run: u32,
    /// The comparisons it made that decided where it went ("18 >= 0 ✓"),
    /// in order.
    pub tests: Vec<String>,
    /// The call returns after it: what it returned (32 bits).
    pub ret: Option<i32>,
    /// It made a call: the function, and what it returned.
    pub called: Option<(String, i32)>,
    /// Inlined: the line of its function's own where the code is inlined
    /// (the outermost inline site: `CLASS(fd_pos, f)(fd)`'s line).
    pub site: Option<u32>,
    /// Inlined code that calls nothing before its function's code goes on
    /// (a helper's lines: task_pid, read_ti_thread_flags): stepping every
    /// line passes over it, staying on the line that used it.
    pub leaf: bool,
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
    /// The syscall entry (arch::SYSCALL): the syscall's name; its
    /// arguments are the syscall's, typed as its parameters.
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

/// A store in a span of records, as the code names what it hit: `what`
/// ("vfs_read's file", "f's x", "nr_inodes", "current"; the address when
/// nothing names it), its `kind` ("task", "object": a struct a call's
/// argument points to, "global", "machine": one of the machine's
/// variables (arch::MACHINE),
/// "frame": the stack a call used, "local", or "other": memory nothing
/// names), the object's type `ty` ("" for none; `id` its id), where it is
/// (`addr`, `size`), and the `write` itself (the member, from what to
/// what, where).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Stored {
    pub what: String,
    pub kind: &'static str,
    pub ty: String,
    pub id: u32,
    pub addr: u32,
    pub size: u32,
    pub write: Change,
    /// For a call's local or frame (or an object in one): the call's
    /// records, its life.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub life: Option<(usize, usize)>,
}

/// An object a span of records wrote (see View::objects): as the Stores
/// name it (`what`, `kind`, `ty`, `id`, `addr`, `size`), its writes in
/// order, and its pointers to the others (as memory is at the cursor, or
/// for an object on the stack whose call has ended by then, as it ended).
/// `life`: an object on the stack lives while its call runs (these
/// records); None: all the recording (as far as it knows).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Node {
    pub what: String,
    pub kind: &'static str,
    pub ty: String,
    pub id: u32,
    pub addr: u32,
    pub size: u32,
    pub writes: Vec<Change>,
    pub links: Vec<Link>,
    pub life: Option<(usize, usize)>,
}

/// A pointer from a node's `member` to node `to` (an index); its value
/// `value`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Link {
    pub member: String,
    pub to: usize,
    pub value: u32,
}

/// A write to a value (`addr`): at record `i`, on `task`, by the code at
/// `at` (the line, if known); `old` what it overwrote (printed: a number,
/// in hex past 0xffff); `from`: if it
/// copied what was at another address (a value loaded and stored as it
/// is, a memcpy), that address.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Write {
    pub i: usize,
    pub addr: u32,
    pub old: String,
    pub task: TaskRef,
    pub at: Option<Line>,
    pub from: Option<u32>,
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

/// The starts of the names of the constants that fit syscall `syscall`'s
/// parameter `param` (arch::SYSCALL_CONSTS).
fn consts_of(syscall: &str, param: &str) -> &'static [&'static str] {
    let key = format!("{syscall}.{param}");
    let consts = arch::SYSCALL_CONSTS;
    consts.iter().find(|(k, _)| *k == key).or_else(|| consts.iter().find(|(k, _)| *k == param)).map_or(&[], |(_, c)| c)
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

/// A task's fields: the key ones (arch::TASK_FIELDS, in that order), or
/// all; `total`: how many it has.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct TaskFields {
    pub fields: Vec<Object>,
    pub total: usize,
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

    /// The fields of the task at `addr`, in `mem` (memory as it was): the
    /// key ones, or `all`.
    pub fn task_fields(&self, mem: &dyn Memory, addr: u32, all: bool) -> TaskFields {
        let types = &self.tables.types;
        let every = types.find(arch::TASK).and_then(|ty| types.object(mem, "current", addr, ty, 1).children).unwrap_or_default();
        let total = every.len();
        let fields = if all { every } else {
            arch::TASK_FIELDS.iter().filter_map(|&k| every.iter().find(|o| o.name == k).cloned()).collect()
        };
        TaskFields { fields, total }
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

    /// Where the kernel was at record `i`: a block's line; for a store, its
    /// line; for a return, the line its frame was at, the record before
    /// (where it returned from); else the line its task was at.
    pub fn at(&self, i: usize) -> Option<At> {
        let t = self.tables;
        let (records, tasks) = (&self.rec.records, &self.rec.tasks);
        // (The innermost call of `i`'s task at record `j`, and the line it was at.)
        let top = |j: usize| self.rec.calls.stack_at(t, j, None).pop().map(|c| (c.func, c.step));
        let located = |func: u32, step: Option<Step>| (step.map(|s| s.loc).or(t.functions.get(&func).map(|f| f.loc)), step.and_then(|s| s.inlined));
        let (kind, func, (loc, inlined)) = match *records.get(i)? {
            Record::Block { func, n } => {
                let inlined = t.segments(func, n).first().and_then(|s| s.first()).and_then(|s| s.inlined);
                ("block", func, (t.blocks.get(&(func, n)).copied(), inlined))
            }
            Record::Return { func, .. } => {
                let prev = (0..i).rev().find(|&j| tasks[j] == tasks[i]);
                ("return", func, located(func, prev.and_then(top).and_then(|(_, s)| s)))
            }
            // (A store, a comparison: on its line, the record's, see line_at.)
            r @ (Record::Store { .. } | Record::Compare { .. }) => {
                let kind = if matches!(r, Record::Store { .. }) { "store" } else { "compare" };
                match self.rec.line_at(t, i) {
                    Some(l) => (kind, l.func, (Some(l.loc), l.inlined)),
                    None => (kind, 0, (None, None)),
                }
            }
            r @ (Record::Task { .. } | Record::Copy { .. }) => {
                let (func, step) = top(i).unwrap_or((0, None));
                (if matches!(r, Record::Task { .. }) { "task" } else { "copy" }, func, located(func, step))
            }
        };
        Some(At {
            i, kind,
            func: t.functions.get(&func).map_or(String::new(), |f| f.name.clone()),
            file: loc.map(|l| t.file(l).to_string()),
            line: loc.map(|l| l.line),
            inlined: self.inlined_from(inlined),
            task: self.task(self.rec.tasks[i]),
            syscall: self.rec.syscall_at(i),
            value: match records[i] { Record::Return { value, .. } => Some(value as i32), _ => None },
        })
    }

    /// Records `from..=to` as text, a line each, as the recording has them
    /// (the raw trace): "call ksys_read(fd=3, buf=0x1cefed0, count=64)
    /// fs/read_write.c:710 frame=0x1ce000" (the block 0 and the Args record
    /// after it, as one line), "block ksys_read #3 fs/read_write.c:712",
    /// "store 0x1ce0020 [8] 0 → 26", "return ksys_read = 26"; a task switch
    /// as a line of its own. Each with its kind: "call", "block", "store",
    /// "return", "task".
    pub fn raw(&self, from: usize, to: usize) -> Vec<(usize, &'static str, String)> {
        let (t, rec) = (self.tables, self.rec);
        let mut out = Vec::new();
        for i in from..=to.min(last_record(rec)) {
            if i > 0 && rec.tasks.get(i) != rec.tasks.get(i - 1) {
                let task = self.task(rec.tasks[i]);
                out.push((i, "task", format!("── {}({}) runs", task.comm.unwrap_or_default(), task.pid.unwrap_or(0))));
            }
            let (kind, text) = match rec.records[i] {
                Record::Block { func, n } => {
                    let loc = t.blocks.get(&(func, n)).map_or(String::new(), |l| format!(" {}:{}", t.file(*l), l.line));
                    // (A call: with its arguments, the record after.)
                    if let (0, Some((words, frame))) = (n, rec.args.get(i)) {
                        let params = t.types.params.get(&func);
                        let each = t.values(func, words).into_iter().enumerate().map(|(k, v)| {
                            let name = params.and_then(|p| p.get(k)).map_or(format!("{k}"), |p| p.0.clone());
                            format!("{name}={}", num(v))
                        });
                        ("call", format!("call {}({}){loc} frame={frame:#x}", self.name(func), each.collect::<Vec<_>>().join(", ")))
                    } else {
                        ("block", format!("block {} #{n}{loc}", self.name(func)))
                    }
                }
                Record::Return { func, value } => ("return", format!("return {} = {}", self.name(func), value as i32)),
                Record::Task { .. } => continue, // (the task's line is above)
                Record::Copy { dst, src, len } => ("copy", format!("copy {dst:#x} ← {src:#x} [{len}]")),
                r @ Record::Compare { .. } => ("compare", format!("compare {}", compared(&r).unwrap_or_default())),
                Record::Store { addr, size: 0, old, .. } => ("store", format!("store {addr:#x} [{old} bytes] (old contents not kept)")),
                Record::Store { addr, size, old, .. } => ("store", format!("store {addr:#x} [{size}] {} → {}", num(old), num(self.wrote(i)))),
            };
            out.push((i, kind, text));
        }
        out
    }

    fn line(&self, s: &LineStep) -> Line {
        let t = self.tables;
        // (Its comparisons: among its stretch's records, those on it.)
        let (records, tasks) = (&self.rec.records, &self.rec.tasks);
        let task = tasks.get(s.record).copied();
        let tests = (s.record + 1..records.len()).filter(|&j| Some(tasks[j]) == task)
            .map(|j| records[j]).take_while(|r| matches!(r, Record::Store { .. } | Record::Copy { .. } | Record::Compare { .. }))
            .filter(|r| matches!(r, Record::Compare { line, .. } if *line == s.index))
            .filter_map(|r| compared(&r)).collect();
        Line { i: s.record, at: s.at, nth: s.nth, func: self.name(s.func), file: t.file(s.loc).to_string(), line: s.loc.line,
               inlined: self.inlined_from(s.inlined), depth: s.depth, inl: self.chain(s.inlined).len() as u32, run: s.run, tests, ret: s.ret.map(|v| v as i32),
               called: s.callee.map(|(f, v)| (self.name(f), v as i32)), site: self.chain(s.inlined).last().map(|&(_, l)| l.line), leaf: false }
    }

    /// The source lines run in records `first..=last` of the task at `task`.
    pub fn lines(&self, first: usize, last: usize, task: u32) -> Vec<Line> {
        let mut out: Vec<Line> = self.rec.lines(self.tables, first, last, task).iter().map(|s| self.line(s)).collect();
        // (Inlined code is a leaf if no call is made before its run ends:
        // before the steps come back out of it, or out of its call.)
        for k in 0..out.len() {
            let (d, inl) = (out[k].depth, out[k].inl);
            if inl == 0 {
                continue;
            }
            let calls = out[k + 1..].iter().take_while(|x| x.depth > d || x.depth == d && x.inl >= inl).any(|x| x.depth > d);
            out[k].leaf = !calls;
        }
        out
    }

    /// The call stack at record `i`, innermost first; locals read from `mem`
    /// (memory as it was then). A call is where its last line is: at `i`,
    /// the `nth` line that record runs (see lines), or its last. Code
    /// inlined there is a frame of its own for each function it is inlined
    /// from (`inlined`, with no arguments: there was no call).
    pub fn call_stack(&self, i: usize, nth: Option<usize>, mem: &dyn Memory) -> Vec<Frame> {
        let t = self.tables;
        let records = &self.rec.records;
        let mut out = Vec::new();
        let entry = t.find(arch::SYSCALL);
        let calls = self.rec.calls.stack_at(t, i, nth);
        let args_of = |c: &stack::StackCall| self.rec.args.get(c.first).map(|(w, frame)| (t.values(c.func, w), frame));
        // (The entry's arguments, the syscall's: a wrapper's parameters are
        // those, as its SYSCALL_DEFINE line has them; its own DWARF one is regs.)
        let entry_raw = calls.iter().find(|c| Some(c.func) == entry).and_then(args_of).map(|(a, _)| a);
        for (depth, c) in calls.into_iter().enumerate().rev() {
            let depth = depth as u32;
            let name = self.name(c.func);
            let (raw, frame) = args_of(&c).unzip();
            let locals = frame.map(|f| self.locals(c.func, f, mem, c.first, i));
            // (The syscall entry: its arguments are the syscall's.)
            let at_entry = matches!(records.get(c.first), Some(&Record::Block { func, n: 0 }) if Some(func) == entry);
            let wrapper = name.starts_with(arch::SYSCALL_WRAPPER);
            let (syscall, args) = match raw {
                Some(raw) => match at_entry.then(|| self.syscall_args(&raw, mem)).flatten() {
                    Some((n, a)) => (Some(n), Some(a)),
                    None if wrapper => (None, Some(entry_raw.as_ref().and_then(|e| self.syscall_args(e, mem)).map_or_else(|| self.args(c.func, raw, mem), |(_, a)| a))),
                    None => (None, Some(self.args(c.func, raw, mem))),
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

    /// The syscall entry's arguments (the number, then the syscall's): the
    /// syscall's name, and its arguments as its parameters have them (with
    /// the constants that name them).
    fn syscall_args(&self, entry: &[u64], mem: &dyn Memory) -> Option<(String, Vec<Object>)> {
        let types = &self.tables.types;
        let name = types.syscalls.get(&(*entry.first()? as u32))?;
        let params = types.syscall_params(name)?;
        let args = params.iter().zip(entry.iter().skip(1).copied()).map(|((pname, ty), v)| {
            let mut o = types.object(&Arg(&v.to_le_bytes(), mem), pname, 0, *ty, 0);
            let names = constant_name(&types.constants, consts_of(name, pname), v as i32 as i64);
            o.value = names.or(o.value);
            o
        }).collect();
        Some((name.clone(), args))
    }

    /// Function `func`'s arguments, their values `args`: the source's
    /// parameters, as values of their types.
    fn args(&self, func: u32, args: Vec<u64>, mem: &dyn Memory) -> Vec<Object> {
        let t = self.tables;
        let known = t.functions.get(&func).map_or(u32::MAX, |f| f.known);
        t.types.params.get(&func).into_iter().flatten().zip(args).enumerate().map(|(k, ((pname, ty), v))| {
            let arg = t.types.object(&Arg(&v.to_le_bytes(), mem), pname, 0, *ty, 0);
            // (Not passed any more: the optimizer took it away.)
            if known >> k & 1 == 0 {
                return Object { value: Some("(optimized out)".into()), children: None, expand: None, target: None, ..arg };
            }
            arg
        }).collect()
    }

    /// Function `func`'s locals in memory, at their offsets from `frame`, as
    /// the call from record `first` has them at record `at`: one whose slot
    /// it has not stored to yet is not written yet (its value then is its
    /// slot's leftovers, another call's: the variable, if it is live, is in
    /// a register meanwhile).
    fn locals(&self, func: u32, frame: u32, mem: &dyn Memory, first: usize, at: usize) -> Vec<Object> {
        let types = &self.tables.types;
        types.locals.get(&func).into_iter().flatten().map(|(var, ty, off)| {
            let addr = frame.wrapping_add(*off);
            let o = types.object(mem, var, addr, *ty, 0);
            if self.rec.stores().written(addr, types.size_of(*ty).max(1), first, at) {
                return o;
            }
            Object { value: Some("(not written yet)".into()), children: None, expand: None, target: None, ..o }
        }).collect()
    }

    /// Call `c` and the code inlined into it where it is, innermost first:
    /// each inlined function (Some: its index, see inline_name) at its line
    /// there, then the call (None) at its own line (where the outermost
    /// inlined code is inlined, or the line it is at).
    pub fn nest(&self, c: &stack::StackCall) -> Vec<(Option<u32>, Option<Loc>)> {
        let mut at = c.step.map(|s| s.loc).or(self.tables.functions.get(&c.func).map(|f| f.loc));
        let mut out = Vec::new();
        for &(k, site) in self.chain(c.step.and_then(|s| s.inlined)) {
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
        let from = self.rec.copied_from(i, addr);
        Some(Write { i, addr, old: num(old), task: self.task(self.rec.tasks[i]), at: line.map(|s| self.line(&s)), from })
    }

    /// Where the value at `addr..addr + len` after record `at` came from:
    /// its newest write, and while that copied it from elsewhere, the
    /// newest write there before it, and so on (up to `max`): the value's
    /// way through memory, newest first. (The last one computed it, or
    /// copied it from memory the recording did not see written.)
    pub fn origin(&self, addr: u32, len: u32, at: usize, max: usize) -> Vec<Write> {
        let mut out: Vec<Write> = Vec::new();
        let (mut addr, mut at) = (addr, at);
        while out.len() < max {
            let Some(w) = self.last_write(addr, len, at) else { break };
            let (i, from) = (w.i, w.from);
            out.push(w);
            let (Some(src), Some(before)) = (from, i.checked_sub(1)) else { break };
            (addr, at) = (src, before);
        }
        out
    }

    /// An object's history: the newest `limit` stores to the value of type
    /// `ty` at `addr` in records `span` (its lifetime), oldest first, each
    /// with the member it wrote (the one its first byte is in: see
    /// Types::leaf), as it was before and after.
    pub fn writes(&self, addr: u32, ty: u32, span: core::ops::RangeInclusive<usize>, limit: usize) -> Vec<Change> {
        let all = self.rec.writes(addr, self.tables.types.size_of(ty).max(1), span);
        all[all.len().saturating_sub(limit)..].iter().map(|&i| self.change(addr, ty, i)).collect()
    }

    /// What the store at record `i` wrote: memory there just after it.
    fn wrote(&self, i: usize) -> u64 {
        let Record::Store { addr, size, .. } = self.rec.records[i] else { return 0 };
        let mut new = [0u8; 8];
        self.rec.memory_at(self.now, i).read(addr, &mut new[..size.min(8) as usize]);
        u64::from_le_bytes(new)
    }

    /// Store `i` as a change to the object of type `ty` at `addr`.
    fn change(&self, addr: u32, ty: u32, i: usize) -> Change {
        let types = &self.tables.types;
        let Record::Store { addr: a, old, .. } = self.rec.records[i] else { unreachable!() };
        let task = self.task(self.rec.tasks[i]);
        let at = self.rec.line_at(self.tables, i).map(|s| self.line(&s));
        // (No type, a frame's bytes: the store as it is, at its offset.)
        if ty == 0 {
            let member = if a == addr { String::new() } else { format!("+{}", a.wrapping_sub(addr)) };
            return Change { i, member, before: num(old), after: num(self.wrote(i)), task, at };
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

    /// The stores in records `first..=last` (a call's), in order, each as
    /// the code names what it hit (see Stored): a call's pointer argument
    /// to a struct, a local, the task running, a callee's frame (from its
    /// frame base up to its caller's: the stack it used beyond its named
    /// locals; no type), a global of the kernel's; else by its address.
    pub fn stores(&self, first: usize, last: usize) -> Vec<Stored> {
        let (t, types) = (self.tables, &self.tables.types);
        // (The objects: what (as the code names it: "vfs_read's file"),
        // type, address, size, and for a call's local or frame its life (the
        // call's records: its slots are other calls' before and after). The
        // task first.)
        let task = self.rec.tasks.get(first).copied().unwrap_or(0);
        let mut regions: Vec<(String, &'static str, u32, u32, u32, Option<(usize, usize)>)> = Vec::new();
        let mut seen = alloc::collections::BTreeSet::new();
        let mut add = |what: String, kind: &'static str, ty: u32, addr: u32, size: Option<u32>, call: Option<usize>| {
            let ty = types.resolve(ty);
            if addr != 0 && seen.insert((addr, ty, call)) {
                let life = call.map(|c| (c, self.rec.calls.call_end(c).unwrap_or(usize::MAX)));
                regions.push((what, kind, ty, addr, size.unwrap_or_else(|| types.size_of(ty)), life));
            }
        };
        if let Some(ty) = types.find(arch::TASK) {
            add("current".into(), "task", ty, task, None, None);
            // (And what its pointer members point to, as the span ends:
            // current->files, ->fs, ->mm: "files->next_fd" through a local
            // the optimizer kept in a register is reached this way.)
            let end = self.rec.memory_at(self.now, last.min(last_record(self.rec)));
            if let Kind::Struct(members) = &types.get(types.resolve(ty)).kind {
                for mb in members.iter().filter(|mb| mb.bit_size == 0) {
                    if let Kind::Pointer(to) = types.get(types.resolve(mb.ty)).kind
                        && matches!(types.get(types.resolve(to)).kind, Kind::Struct(_) | Kind::Union(_)) {
                        add(format!("current->{}", mb.name), "object", to, end.read_u32(task.wrapping_add(mb.bit_offset / 8)), None, None);
                    }
                }
            }
        }
        let (records, calls, args) = (&self.rec.records, &self.rec.calls, &self.rec.args);
        // (The call whose frame holds address `p`, seen from the call at
        // record `i`: an object a pointer reaches on the stack lives as long
        // as that call, the slot another's before and after; None: not on
        // the stack.)
        let frame_of = |i: usize, p: u32| {
            let mut c = calls.call_from(i)?;
            loop {
                let parent = &calls.calls[c.parent? as usize];
                let (below, above) = (args.get(c.first)?.1, args.get(parent.first)?.1);
                if below <= p && p < above {
                    return Some(c.first);
                }
                c = parent;
            }
        };
        for i in first..=last.min(records.len().saturating_sub(1)) {
            match records[i] {
                Record::Block { func, n: 0 } => {
                    let Some((f, (words, frame))) = t.functions.get(&func).zip(args.get(i)) else { continue };
                    // (Its frame: up to its caller's, if the caller is known.)
                    let caller = calls.call_from(i).and_then(|c| c.parent).and_then(|p| args.get(calls.calls[p as usize].first));
                    if let Some((_, caller)) = caller && caller > frame {
                        add(format!("{}'s frame", f.name), "frame", 0, frame, Some(caller - frame), Some(i));
                    }
                    for ((pname, ty), v) in types.params.get(&func).into_iter().flatten().zip(t.values(func, words)) {
                        let v = v as u32;
                        if let Kind::Pointer(to) = types.get(types.resolve(*ty)).kind
                            && matches!(types.get(types.resolve(to)).kind, Kind::Struct(_) | Kind::Union(_)) {
                            add(format!("{}'s {pname}", f.name), "object", to, v, None, frame_of(i, v));
                        }
                    }
                    for (var, ty, off) in types.locals.get(&func).into_iter().flatten() {
                        add(format!("{}'s {var}", f.name), "local", *ty, frame.wrapping_add(*off), None, Some(i));
                        // (A local pointing at a struct: that too ("files->next_fd"
                        // through `struct files_struct *files = current->files`),
                        // where it points as the call ends.)
                        if let Kind::Pointer(to) = types.get(types.resolve(*ty)).kind
                            && matches!(types.get(types.resolve(to)).kind, Kind::Struct(_) | Kind::Union(_)) {
                            let end = self.rec.calls.call_end(i).unwrap_or(last).min(last_record(self.rec));
                            let v = self.rec.memory_at(self.now, end).read_u32(frame.wrapping_add(*off));
                            add(format!("{}'s {var}", f.name), "object", to, v, None, frame_of(i, v));
                        }
                    }
                }
                _ => {}
            }
        }
        // (The kernel's globals, by address, for the stores nothing else holds.)
        let mut globals: Vec<(u32, u32, &str, u32)> = types.globals.iter()
            .map(|(name, &(addr, ty))| (addr, types.size_of(ty), name.as_str(), ty)).filter(|g| g.1 > 0).collect();
        globals.sort_unstable();
        let global_at = |addr: u32| {
            let k = globals.partition_point(|g| g.0 <= addr);
            globals[..k].iter().rev().take(8).find(|g| addr < g.0.saturating_add(g.1)).copied()
        };
        // (Each store to the smallest object holding its first byte.)
        let mut out = Vec::new();
        for &i in self.rec.stores_between(first, last) {
            let Record::Store { addr, .. } = records[i] else { continue };
            let hit = regions.iter().filter(|r| r.3 <= addr && addr < r.3.saturating_add(r.4) && r.5.is_none_or(|(a, b)| a <= i && i <= b))
                .min_by_key(|r| r.4).cloned()
                .or_else(|| global_at(addr).map(|(gaddr, size, name, ty)| {
                    let machine = types.declared.get(name).is_some_and(|(f, _)| f.starts_with(arch::MACHINE));
                    (name.into(), if machine { "machine" } else { "global" }, types.resolve(ty), gaddr, size, None)
                }));
            out.push(match hit {
                Some((what, kind, ty, base, size, life)) => Stored { what, kind, ty: types.name(ty), id: ty, addr: base, size, write: self.change(base, ty, i), life },
                None => Stored { what: format!("{addr:#x}"), kind: "other", ty: String::new(), id: 0, addr, size: 0, write: self.change(addr, 0, i), life: None },
            });
        }
        out
    }

    /// The objects records `first..=last` (a call's) wrote, as a graph: each
    /// with its writes (the Stores', by object, in the order first written),
    /// and its pointers to the others as memory was after record `at`
    /// (the cursor; an object on the stack, when its call ended if that
    /// was before): what the call changed in the kernel's data, and how it
    /// hangs together. (Not the frames or the machine's own
    /// variables: the call's plumbing.)
    pub fn objects(&self, first: usize, last: usize, at: usize) -> Vec<Node> {
        let types = &self.tables.types;
        let mut nodes: Vec<Node> = Vec::new();
        for s in self.stores(first, last).into_iter().filter(|s| matches!(s.kind, "object" | "task" | "local" | "global") && s.size > 0) {
            match nodes.iter_mut().find(|n| n.addr == s.addr && n.id == s.id && n.writes[0].i >= s.life.map_or(0, |l| l.0)) {
                Some(n) => n.writes.push(s.write),
                None => nodes.push(Node { what: s.what, kind: s.kind, ty: s.ty, id: s.id, addr: s.addr, size: s.size, writes: alloc::vec![s.write], links: Vec::new(), life: s.life }),
            }
        }
        let within = |p: u32| nodes.iter().position(|n| n.addr <= p && p < n.addr.saturating_add(n.size));
        let links: Vec<Vec<Link>> = nodes.iter().enumerate().map(|(k, n)| {
            let mem = self.rec.memory_at(self.now, n.life.map_or(at, |l| l.1.min(at)));
            types.pointers(n.id, 3).into_iter().filter_map(|(member, off)| {
            let value = mem.read_u32(n.addr.wrapping_add(off));
            let to = within(value).filter(|&to| to != k)?;
            Some(Link { member, to, value })
            }).collect()
        }).collect();
        for (n, l) in nodes.iter_mut().zip(links) {
            n.links = l;
        }
        nodes
    }

    /// The task running at the start of each of `n` buckets of the recording.
    pub fn timeline(&self, n: usize) -> Vec<TaskRef> {
        let steps = self.rec.steps().max(1);
        (0..n).filter_map(|b| {
            let i = self.rec.record_of(b * steps / n);
            Some(self.task(*self.rec.tasks.get(i)?))
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
    /// recording), with how often (the most of the blocks running it): the
    /// lines each block runs (Tables::runs: inlined code's, and where it is
    /// inlined, too).
    pub fn coverage(&self, file: &str, n: Option<usize>) -> Vec<LineCount> {
        let t = self.tables;
        let counts = self.rec.block_counts(n);
        let files: Vec<u32> = (0..t.files.len() as u32).filter(|&k| t.files[k as usize] == file).collect();
        let mut lines: BTreeMap<u32, u64> = BTreeMap::new();
        for (&(f, b), &c) in counts.iter().filter(|(_, c)| **c > 0) {
            for loc in t.runs(f, b).filter(|l| files.contains(&l.file)) {
                let e = lines.entry(loc.line).or_default();
                *e = (*e).max(c);
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
                match self.get(target).kind {
                    Kind::Struct(_) | Kind::Union(_) if p != 0 => o.target = Some(target),
                    // (A pointer to a scalar: what it points to, in line: "0x7ffd8 → 0".)
                    Kind::Base(_) | Kind::Enum(_) | Kind::Pointer(_) if p != 0 && !self.is_string(*to) => {
                        let mut v = String::new();
                        self.print(mem, p, *to, 0, 0, &mut v);
                        o.value = Some(format!("{p:#x} → {v}"));
                    }
                    _ => {}
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

/// A number as the page shows one: in hex past 0xffff (an address, a flag
/// word).
fn num(v: u64) -> String {
    if v > 0xffff { format!("{v:#x}") } else { v.to_string() }
}

/// A comparison, its operands and whether it held: "18 >= 0 ✓" (signed
/// ones as such, else as numbers are).
fn compared(r: &Record) -> Option<String> {
    let (op, signed, held) = r.compare()?;
    let &Record::Compare { lhs, rhs, .. } = r else { return None };
    let v = |x: u32| if signed { (x as i32).to_string() } else { num(x as u64) };
    Some(format!("{} {op} {} {}", v(lhs), v(rhs), if held { "✓" } else { "✗" }))
}

/// The last record's index (0 if none).
fn last_record(rec: &Recording) -> usize {
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
    use crate::trace::Args;

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
        let r = [Record::Block { func: 9, n: 1 }, Record::Store { addr: 8, size: 4, old: 1, line: 0 },
                 Record::Block { func: 9, n: 1 }, Record::Store { addr: 5, size: 1, old: 0, line: 0 }];
        let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 4], Args::default());
        let mut m = alloc::vec![0u8; 16];
        (m[8], m[5]) = (2, 5);
        let v = View { tables: &t, rec: &rec, now: &Mem(m) };
        let got: Vec<_> = v.writes(4, 2, 0..=3, 8).into_iter().map(|c| (c.i, c.member, c.before, c.after)).collect();
        assert_eq!(got, [(1, "b".into(), "1".into(), "2".into()), (3, "a".into(), "0".into(), "1280".into())]);
        assert_eq!(v.writes(4, 2, 2..=3, 8).len(), 1);
    }

    #[test]
    fn a_calls_stores_are_named_as_the_code_names_them() {
        // f (1, "f") takes struct s *p (at 4) and has a local int x (at
        // frame 32 + 4): it stores p->b, then x; calls g (2), whose frame is
        // at 16, which stores a byte of its own frame; and one byte nobody
        // names.
        let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\nF\t2\tg\t0:9\t0\n",
                              "T\t1\tbase\tint\t4\ts\nT\t2\tstruct\ts\t8\nM\t2\ta\t0\t0\t1\nM\t2\tb\t32\t0\t1\nT\t3\tptr\t\t4\t2\n\
                               P\t1\tf\tp:3\nQ\t1\tx:1:4\n");
        let r = [Record::Block { func: 1, n: 0 },
                 Record::Store { addr: 8, size: 4, old: 1, line: 0 }, Record::Store { addr: 36, size: 4, old: 0, line: 0 },
                 Record::Block { func: 2, n: 0 },
                 Record::Store { addr: 20, size: 1, old: 0, line: 0 }, Record::Return { func: 2, value: 0 },
                 Record::Store { addr: 100, size: 1, old: 0, line: 0 }, Record::Store { addr: 21, size: 1, old: 0, line: 0 }];
        let mut args = Args::default();
        args.push(0, &[4, 32]);
        args.push(3, &[16]);
        let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 8], args);
        let mut m = alloc::vec![0u8; 128];
        (m[8], m[36], m[20], m[100]) = (2, 7, 9, 5);
        let v = View { tables: &t, rec: &rec, now: &Mem(m) };
        // (The last, to g's frame after g returned: g's no more.)
        let got: Vec<_> = v.stores(0, 7).into_iter().map(|s| (s.what, s.kind, s.write.member, s.write.after)).collect();
        assert_eq!(got, [("f's p".into(), "object", "b".into(), "2".into()), ("f's x".into(), "local", "".into(), "7".into()),
                         ("g's frame".into(), "frame", "+4".into(), "9".into()), ("0x64".into(), "other", "".into(), "5".into()),
                         ("0x15".into(), "other", "".into(), "0".into())]);
    }

    #[test]
    fn the_objects_a_call_wrote_are_a_graph() {
        // f (1) takes struct s *p (at 4) and has a local struct t y (at frame
        // 32 + 8: int c, then struct s *link): it stores p->b and y.c, and
        // y.link points at *p: two nodes, y linked to p's.
        let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\n",
                              "T\t1\tbase\tint\t4\ts\nT\t2\tstruct\ts\t8\nM\t2\ta\t0\t0\t1\nM\t2\tb\t32\t0\t1\nT\t3\tptr\t\t4\t2\n\
                               T\t4\tstruct\tt\t8\nM\t4\tc\t0\t0\t1\nM\t4\tlink\t32\t0\t3\nP\t1\tf\tp:3\nQ\t1\ty:4:8\n");
        let r = [Record::Block { func: 1, n: 0 }, Record::Store { addr: 8, size: 4, old: 1, line: 0 }, Record::Store { addr: 40, size: 4, old: 0, line: 0 }];
        let mut args = Args::default();
        args.push(0, &[4, 32]);
        let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 3], args);
        let mut m = alloc::vec![0u8; 64];
        (m[8], m[40], m[44]) = (2, 7, 4);
        let v = View { tables: &t, rec: &rec, now: &Mem(m) };
        let nodes = v.objects(0, 2, 2);
        let got: Vec<_> = nodes.iter().map(|n| (n.what.clone(), n.writes.len(), n.links.iter().map(|l| (l.member.clone(), l.to)).collect::<Vec<_>>())).collect();
        assert_eq!(got, [("f's p".into(), 1, alloc::vec![]), ("f's y".into(), 1, alloc::vec![("link".into(), 0)])]);
    }

    #[test]
    fn a_value_is_followed_through_its_copies() {
        // 7 stored at 0x30 (computed), then copied to 0x10 (a load, a store),
        // then to 0x20 by a memcpy of 8 (two stores): 0x24's way back is
        // 0x24 <- 0x14 <- ... 0x14 was never written; 0x20's is 0x20 <- 0x10 <- 0x30.
        let t = Tables::default();
        let st = |addr| Record::Store { addr, size: 4, old: 0, line: 0 };
        let r = [st(0x30), Record::Copy { dst: 0x10, src: 0x30, len: 4 }, st(0x10),
                 Record::Copy { dst: 0x20, src: 0x10, len: 8 }, st(0x20), st(0x24)];
        let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 6], Args::default());
        let v = View { tables: &t, rec: &rec, now: &Mem(alloc::vec![0; 64]) };
        let way = |a| v.origin(a, 4, 5, 8).iter().map(|w| (w.i, w.addr, w.from)).collect::<Vec<_>>();
        assert_eq!(way(0x20), [(4, 0x20, Some(0x10)), (2, 0x10, Some(0x30)), (0, 0x30, None)]);
        assert_eq!(way(0x24), [(5, 0x24, Some(0x14))]);
    }

    #[test]
    fn frames_are_where_their_calls_are() {
        // f (1) runs lines 1 and 2, calls g (2) at 2, then runs 3.
        let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\nF\t2\tg\t0:9\t0\nB\t1\t0\t0:1:1\n\
                               L\t1\t0\t0:1;:2;|;:3\nB\t2\t0\t0:10:1\n", "");
        let r = [Record::Block { func: 1, n: 0 }, Record::Block { func: 2, n: 0 }];
        let rec = Recording::new(&t, r.to_vec(), alloc::vec![0, 1], Args::default());
        let v = View { tables: &t, rec: &rec, now: &Mem(Vec::new()) };
        let lines: Vec<_> = v.call_stack(1, None, &Mem(Vec::new())).iter().map(|f| (f.func.clone(), f.line)).collect();
        assert_eq!(lines, [("g".into(), Some(10)), ("f".into(), Some(2))]);
    }

    #[test]
    fn inlined_code_is_a_frame_of_its_own() {
        // f (1) runs line 1, then h's line 20 (h inlined into f at line 2),
        // which calls g (2).
        let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\nF\t2\tg\t0:9\t0\nB\t1\t0\t0:1:1\n\
                               I\t0\th\t0:2\nL\t1\t0\t0:1;:20@0;|\nB\t2\t0\t0:10:1\n", "");
        let r = [Record::Block { func: 1, n: 0 }, Record::Block { func: 2, n: 0 }];
        let rec = Recording::new(&t, r.to_vec(), alloc::vec![0, 1], Args::default());
        let v = View { tables: &t, rec: &rec, now: &Mem(Vec::new()) };
        let frames: Vec<_> = v.call_stack(1, None, &Mem(Vec::new())).iter().map(|f| (f.func.clone(), f.line, f.inlined)).collect();
        assert_eq!(frames, [("g".into(), Some(10), false), ("h".into(), Some(20), true), ("f".into(), Some(2), false)]);
        // (Line 2, where h is inlined, ran too: h's line 20 ran for it.)
        assert!(v.coverage("f.c", None).iter().any(|l| l.line == 2));
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
    fn scalar_pointers_show_what_they_point_to() {
        let t = Types::parse("T\t1\tbase\tint\t4\ts\nT\t2\tptr\t\t4\t1\n");
        let mut m = alloc::vec![0u8; 16];
        m[0..4].copy_from_slice(&8u32.to_le_bytes());
        m[8..12].copy_from_slice(&(-3i32).to_le_bytes());
        let o = t.object(&Mem(m), "pos", 0, 2, 0);
        assert_eq!((o.value.as_deref(), o.target), (Some("0x8 → -3"), None));
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
