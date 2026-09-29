//! Views of a recording, for the page (serializable with the "serde"
//! feature): what it shows, by name rather than by id.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use alloc::collections::BTreeMap;

use crate::arch;
use crate::recording::Recording;
use crate::stack::{self, LineStep};
use crate::tables::{Loc, Step, Tables, Var};
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
    pub value: Option<i64>,
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
    pub ret: Option<i64>,
    /// It made a call: the function, what it returned, and where (its
    /// return: the result is not known before).
    pub called: Option<(String, i64, usize)>,
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

/// What a span's stores hit, as the code names it (see View::stores):
/// `what` ("file in vfs_read", "x in f", "nr_inodes", "prog(19)"; the
/// address when nothing names it), its `kind` ("task", "object": a struct
/// a call's argument points to, "global", "machine": one of the machine's
/// variables, or what its functions name (arch::MACHINE), "frame": the
/// stack a call used, "local", "var": a variable kept in a wasm local, or
/// "other": memory nothing names), its type `ty` (0 for none: a frame's
/// bytes; a var's, the variable's), where it is (`addr`, `size`), for a
/// call's local or frame (or an object in one) the call's records, its
/// `life`, and how deep the call that names it is (the task's, 0; a
/// global's, or memory nothing names, u32::MAX): Objects comes in that
/// order.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub what: String,
    pub kind: &'static str,
    pub ty: u32,
    pub addr: u32,
    pub size: u32,
    pub life: Option<(usize, usize)>,
    pub depth: u32,
}

/// A span's stores and variable sets, in order, by the regions they hit.
#[derive(Debug, Default)]
pub struct Named {
    pub regions: Vec<Region>,
    pub writes: Vec<Wrote>,
}

/// A store or a variable set: at record `i`, to `region`, on line `at`
/// (Recording::line_at); a variable set's `prev`, the set of the same
/// variable before it in the span.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Wrote {
    pub i: usize,
    pub region: u32,
    pub at: Option<LineStep>,
    pub prev: Option<usize>,
}

/// A store in a span (Machine.stores): at record `i`, to the `member` (a
/// path: "f_path.dentry"; "" the object itself; "+8" a frame's byte 8) of
/// `what`, of `kind` (see Region).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Stored {
    pub i: usize,
    pub what: String,
    pub kind: &'static str,
    pub member: String,
}

/// A member's scalar, as Types::leaf finds it: its type, its offset in the
/// object, and its bits if a bitfield.
type Leaf = (u32, u32, Option<(u32, u32)>);

/// A write, printed: the member it wrote (see Stored), from `before` to
/// `after`.
#[derive(Debug, Clone, PartialEq)]
struct Change {
    member: String,
    before: String,
    after: String,
}

/// What a source line stored (or set) by a moment (View::notes): each
/// thing it wrote, said `short` and in `full`; its last write at record
/// `i`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Note {
    pub line: u32,
    pub i: usize,
    pub short: String,
    pub full: String,
}

/// An object a span of records (a call) changed, which is still there when
/// it returns (see View::objects): as the Stores name it (`what`, `kind`,
/// `ty`, `id`, `addr`, `size`), and the members the call wrote.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Node {
    pub what: String,
    pub kind: &'static str,
    pub ty: String,
    pub id: u32,
    pub addr: u32,
    pub size: u32,
    pub changes: Vec<Diff>,
    #[cfg_attr(feature = "serde", serde(skip))]
    pub depth: u32,
    /// What happened to it: "changed" (it ends otherwise than it began:
    /// what the call did), or "same" (every change undone by the end: a
    /// lock taken and released, a count taken and dropped).
    pub group: &'static str,
    /// The node it is shown under (of its group), and by which of its
    /// members: the first that points to it as the span ends ("f_inode"),
    /// if any.
    pub under: Option<(usize, String)>,
}

/// A member a span of records changed: from `before` (as the span began)
/// to `after` (as it ends; the same: left as it was, a count taken and
/// dropped), a pointer into another node as it ends: `to`, its index; and
/// how, in `runs`: a run a function writing it in turn, in order.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Diff {
    pub member: String,
    pub before: String,
    pub after: String,
    pub to: Option<usize>,
    pub runs: Vec<Run>,
}

/// Writes of one function in a row to a member: from `before` to `after`,
/// `writes` of them, the first at record `first`, the last at `last`, by
/// the code at `at`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Run {
    pub before: String,
    pub after: String,
    pub writes: usize,
    pub first: usize,
    pub last: usize,
    pub at: Option<Line>,
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

/// A syscall: its parameters (as SYSCALL_DEFINE declares them).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct SyscallInfo {
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
/// all.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct TaskFields {
    pub fields: Vec<Object>,
}

/// A task the recording has, at a record: whether it is the one running
/// there; its `last` record by then (None: it first runs after, at
/// `first`), and there the syscall it is in, else its outermost call (what
/// it does: off the CPU, its innermost is always the switch).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct TaskAt {
    pub task: TaskRef,
    /// The task it sits under (its task_struct's address): a thread, its
    /// thread group's leader; a process, its parent.
    pub up: Option<u32>,
    pub running: bool,
    pub first: usize,
    pub last: Option<usize>,
    pub syscall: Option<String>,
    pub func: Option<String>,
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
        // (Read once per snapshot: every store names its task.)
        let known = self.rec.names.borrow().get(&addr).cloned();
        let t = known.or_else(|| {
            let t = read_task(self.tables, self.now, addr)?;
            self.rec.names.borrow_mut().insert(addr, t.clone());
            Some(t)
        });
        TaskRef { addr, pid: t.as_ref().map(|t| t.pid), comm: t.map(|t| t.comm) }
    }

    /// The fields of the task at `addr`, in `mem` (memory as it was): the
    /// key ones, or `all`.
    pub fn task_fields(&self, mem: &dyn Memory, addr: u32, all: bool) -> TaskFields {
        let types = &self.tables.types;
        let every = types.find(arch::TASK).and_then(|ty| types.object(mem, arch::CURRENT, addr, ty, 1).children).unwrap_or_default();
        let fields = if all { every } else {
            arch::TASK_FIELDS.iter().filter_map(|&k| every.iter().find(|o| o.name == k).cloned()).collect()
        };
        TaskFields { fields }
    }

    /// The tasks the recording has, in the order they first run, as they
    /// are at record `i` (TaskAt).
    pub fn tasks(&self, i: usize) -> Vec<TaskAt> {
        let t = self.tables;
        // (Each task's first record, and its last by `i`.)
        let mut seen: Vec<(u32, usize, Option<usize>)> = Vec::new();
        for (j, &task) in self.rec.tasks.iter().enumerate().filter(|&(_, &task)| task != 0) {
            let k = match seen.iter().position(|s| s.0 == task) {
                Some(k) => k,
                None => (seen.push((task, j, None)), seen.len() - 1).1,
            };
            if j <= i {
                seen[k].2 = Some(j);
            }
        }
        let field = |addr: u32, f: &str| t.types.field(arch::TASK, f).map(|(off, _)| self.now.read_u32(addr + off)).filter(|&p| p != 0);
        seen.into_iter().map(|(addr, first, last)| TaskAt {
            task: self.task(addr),
            up: field(addr, arch::TASK_LEADER).filter(|&l| l != addr).or_else(|| field(addr, arch::TASK_PARENT)).filter(|&p| p != addr),
            running: self.rec.tasks.get(i) == Some(&addr),
            first, last,
            syscall: last.and_then(|j| self.rec.syscall_at(j)).and_then(|s| t.types.syscalls.get(&self.rec.syscalls[s].nr).cloned()),
            func: last.and_then(|j| self.rec.calls.stack_at(t, j, None).into_iter().next()).and_then(|c| t.functions.get(&c.func)).map(|f| f.name.clone()),
        }).collect()
    }

    pub fn syscalls(&self) -> Vec<SyscallSummary> {
        self.rec.syscalls.iter().enumerate().map(|(index, s)| SyscallSummary {
            index,
            id: self.rec.ids.get(s.first).copied().unwrap_or(0),
            task: self.task(s.task),
            nr: s.nr,
            name: self.tables.types.syscalls.get(&s.nr).cloned(),
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
                let b = t.block(func, n);
                ("block", func, (b.map(|b| b.loc()), b.and_then(|b| b.steps.first()?.inlined)))
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
            r @ (Record::Task { .. } | Record::Var { .. }) => {
                let (func, step) = top(i).unwrap_or((0, None));
                let kind = if matches!(r, Record::Task { .. }) { "task" } else { "var" };
                (kind, func, located(func, step))
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
            value: match records[i] { Record::Return { value, .. } => Some(value), _ => None },
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
                    let loc = t.block(func, n).map_or(String::new(), |b| format!(" {}:{}", t.file(b.loc()), b.loc().line));
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
                Record::Return { func, value } => ("return", format!("return {} = {}", self.name(func), value)),
                Record::Task { .. } => continue, // (the task's line is above)
                // (As its type says: a pointer in hex, a string's text, an enumerator.)
                Record::Var { var, value, .. } => {
                    let v = self.var_at(i, var);
                    let shown = v.and_then(|v| self.var_object(v, value, self.now)).and_then(|o| o.value).unwrap_or_else(|| num(value));
                    ("var", format!("var {} = {shown}", v.map_or("?", |v| v.name.as_str())))
                }
                r @ Record::Compare { .. } => ("compare", format!("compare {}", compared(&r).unwrap_or_default())),
                Record::Store { addr, size, old, from, .. } => {
                    let from = from.map_or(String::new(), |f| format!(" ← {f:#x}"));
                    ("store", match old {
                        None => format!("store {addr:#x} [{size} bytes] (old contents not kept){from}"),
                        Some(old) => format!("store {addr:#x} [{size}] {} → {}{from}", num(old), num(self.wrote(i))),
                    })
                }
            };
            out.push((i, kind, text));
        }
        out
    }

    /// Line step `s` as the page has it, with the comparisons it made.
    fn line(&self, s: &LineStep) -> Line {
        // (Its comparisons: among its stretch's records, those on it.)
        let (records, tasks) = (&self.rec.records, &self.rec.tasks);
        let task = tasks.get(s.record).copied();
        let tests = (s.record + 1..records.len()).filter(|&j| Some(tasks[j]) == task)
            .map(|j| records[j]).take_while(|r| matches!(r, Record::Store { .. } | Record::Compare { .. } | Record::Var { .. }))
            .filter(|r| matches!(r, Record::Compare { line, .. } if *line == s.index))
            .filter_map(|r| compared(&r)).collect();
        Line { tests, ..self.line_only(s) }
    }

    /// Where line step `s` is (a store's, a write's place): the line, not
    /// what it compared.
    fn line_only(&self, s: &LineStep) -> Line {
        Line { i: s.record, at: s.at, nth: s.nth, func: self.name(s.func), file: self.tables.file(s.loc).to_string(), line: s.loc.line,
               inlined: self.inlined_from(s.inlined), depth: s.depth, inl: self.chain(s.inlined).len() as u32, run: s.run, tests: Vec::new(), ret: s.ret,
               called: s.callee.map(|(f, v, at)| (self.name(f), v, at)), site: self.chain(s.inlined).last().map(|&(_, l)| l.line), leaf: false }
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
    /// (`at`: the moment `mem` is memory as it was at, for the variables'
    /// values too: a line step's.)
    pub fn call_stack(&self, i: usize, nth: Option<usize>, mem: &dyn Memory, at: usize) -> Vec<Frame> {
        let t = self.tables;
        let mut out = Vec::new();
        // (At a record on a line of its own, a store, a comparison, a
        // variable set: on that line, as `at` says it.)
        let calls = match nth.is_none().then(|| self.rec.line_at(t, i)).flatten() {
            Some(s) => self.rec.calls.stack_at(t, s.record, Some(s.nth as usize)),
            None => self.rec.calls.stack_at(t, i, nth),
        };
        let args_of = |c: &stack::StackCall| self.rec.args.get(c.first).map(|(w, frame)| (t.values(c.func, w), frame));
        // (The syscall, by its entry's arguments (arch::SYSCALL): its name,
        // and its arguments, the entry's and its wrapper's (a wrapper's
        // parameters are the syscall's, as its SYSCALL_DEFINE line has them;
        // its own DWARF one is regs).)
        let entry = t.find(arch::SYSCALL);
        let syscall = calls.iter().find(|c| Some(c.func) == entry).and_then(args_of).and_then(|(a, _)| self.syscall_args(&a, mem));
        for (depth, c) in calls.into_iter().enumerate().rev() {
            let depth = depth as u32;
            let name = self.name(c.func);
            let (raw, frame) = args_of(&c).unzip();
            let at_entry = Some(c.func) == entry;
            let args = raw.map(|raw| match &syscall {
                Some((_, a)) if at_entry || name.starts_with(arch::SYSCALL_WRAPPER) => a.clone(),
                _ => self.args(c.func, raw, mem),
            });
            let syscall = syscall.as_ref().filter(|_| at_entry && args.is_some()).map(|(n, _)| n.clone());
            // (The variables it keeps in wasm locals, as they are by now: a
            // parameter it changed, as it is; the other locals after those
            // in memory. And the code inlined into it: its own.)
            let vars = self.var_values(&c, at);
            // (Its function's, in the order it declares them.)
            let mine = |function: &str, param: bool| {
                let mut o: Vec<((bool, usize), Object)> = vars.iter().filter(|((f, _, _), (v, _))| *f == function && (v.param > 0) == param)
                    .filter_map(|(_, (v, value))| Some((self.declared(v), self.var_object(v, *value, mem)?))).collect();
                o.sort_by_key(|(k, _)| *k);
                o.into_iter().map(|(_, o)| o).collect::<Vec<_>>()
            };
            let changed = mine(&name, true);
            let args = args.map(|a| a.into_iter().map(|o| changed.iter().find(|p| p.name == o.name).cloned().unwrap_or(o)).collect());
            let locals = frame.map(|f| self.locals(c.func, f, mem, c.first, at, &mine(&name, false)));
            // (The code inlined into it first, innermost first; then it.)
            let frame = |func: String, inlined, at: Option<Loc>| {
                let (a, l) = if inlined { (mine(&func, true), mine(&func, false)) } else { (Vec::new(), Vec::new()) };
                Frame { first: c.first, depth, syscall: None, inlined, file: at.map(|l| t.file(l).to_string()), line: at.map(|l| l.line),
                        args: (!a.is_empty()).then_some(a), locals: (!l.is_empty()).then_some(l), func }
            };
            let nest = self.nest(&c);
            let (&(_, at), inner) = nest.split_last().expect("a call");
            out.extend(inner.iter().map(|&(k, at)| frame(self.inline_name(k.unwrap_or(0)), true, at)));
            out.push(Frame { args, locals, syscall, ..frame(name, false, at) });
        }
        out
    }

    /// Variable `var` of the function running at record `i` (a Var
    /// record's).
    fn var_at(&self, i: usize, var: u32) -> Option<&Var> {
        self.tables.vars.get(&(self.rec.calls.func_at(i)?, var))
    }

    /// The variables call `c` keeps in wasm locals (Record::Var), by record
    /// `at`: each one's last value, by its function (the call's, or one
    /// inlined into it: by name) and its name, parameters by number.
    fn var_values(&self, c: &stack::StackCall, at: usize) -> BTreeMap<(&str, u32, &str), (&Var, u64)> {
        let mut out = BTreeMap::new();
        // (One that is the call's argument throughout: that.)
        let args = self.rec.args.get(c.first).map(|(w, _)| self.tables.values(c.func, w)).unwrap_or_default();
        for (_, v) in self.tables.vars.range((c.func, 0)..=(c.func, u32::MAX)).filter(|(_, v)| v.arg > 0) {
            if let Some(&value) = args.get(v.arg as usize - 1) {
                out.insert((v.function.as_str(), v.param, v.name.as_str()), (v, value));
            }
        }
        for j in self.rec.calls.vars_of(c.first, at) {
            if let Record::Var { var, value, .. } = self.rec.records[j]
                && let Some(v) = self.tables.vars.get(&(c.func, var)) {
                out.insert((v.function.as_str(), v.param, v.name.as_str()), (v, value));
            }
        }
        out
    }

    /// Where variable `v` is among its function's: a parameter by its
    /// number, a local after them, in the order it declares them.
    fn declared(&self, v: &Var) -> (bool, usize) {
        match v.param {
            0 => (true, self.tables.types.locals.get(&v.of).and_then(|l| l.iter().position(|l| l.0 == v.name)).unwrap_or(0)),
            k => (false, k as usize),
        }
    }

    /// Variable `v` with `value`, as a value of its type (its function's
    /// parameter's, or local's).
    fn var_object(&self, v: &Var, value: u64, mem: &dyn Memory) -> Option<Object> {
        Some(self.tables.types.object(&Arg(&value.to_le_bytes(), mem), &v.name, 0, self.var_ty(v)?, 0))
    }

    /// Variable `v`'s type: its function's parameter's, or local's.
    fn var_ty(&self, v: &Var) -> Option<u32> {
        let types = &self.tables.types;
        Some(match v.param {
            0 => types.locals.get(&v.of)?.iter().find(|l| l.0 == v.name)?.1,
            k => types.params.get(&v.of)?.get(k as usize - 1)?.1,
        })
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

    /// Function `func`'s locals, each name once, in the order it declares them (the same
    /// rows at every moment, as a debugger's), as the call from record
    /// `first` has them at record `at`: one in memory at its offset from
    /// `frame`, uninitialized if its slot has not been stored to (its
    /// value then is its slot's leftovers, another call's); one in a wasm
    /// local as `set` has it (Record::Var), else not set yet.
    fn locals(&self, func: u32, frame: u32, mem: &dyn Memory, first: usize, at: usize, set: &[Object]) -> Vec<Object> {
        let types = &self.tables.types;
        let not = |o: Object, why: &str| Object { value: Some(why.into()), children: None, expand: None, target: None, ..o };
        // (A name once: two variables of one name, in two of its scopes, are
        // one to the recording, which knows a variable by its name.)
        let mut seen = alloc::collections::BTreeSet::new();
        types.locals.get(&func).into_iter().flatten().filter(|(var, _, _)| seen.insert(var.as_str())).map(|(var, ty, off)| match off {
            Some(off) => {
                let addr = frame.wrapping_add(*off);
                let o = types.object(mem, var, addr, *ty, 0);
                if self.rec.stores().written(addr, types.size_of(*ty).max(1), first, at) { o } else { not(o, "(uninitialized)") }
            }
            None => set.iter().find(|o| &o.name == var).cloned()
                .unwrap_or_else(|| not(types.object(&Arg(&[0; 8], mem), var, 0, *ty, 0), "(uninitialized)")),
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
        let old = old.map_or_else(|| "(not kept)".into(), num);
        Some(Write { i, addr, old, task: self.task(self.rec.tasks[i]), at: line.map(|s| self.line_only(&s)), from })
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

    /// What the store at record `i` wrote: memory there just after it.
    fn wrote(&self, i: usize) -> u64 {
        let Record::Store { addr, size, .. } = self.rec.records[i] else { return 0 };
        let mut new = [0u8; 8];
        self.rec.memory_at(self.now, i).read(addr, &mut new[..size.min(8) as usize]);
        u64::from_le_bytes(new)
    }

    /// The members of `r` the store at record `i` wrote (none for a
    /// variable set): each its path and, for a typed region, the leaf there
    /// (types::leaves: one store may copy several members, a struct path's
    /// mnt and dentry at once).
    fn members(&self, r: &Region, i: usize) -> Vec<(String, Option<Leaf>)> {
        let Record::Store { addr, size, .. } = self.rec.records[i] else { return Vec::new() };
        // (No type, a frame's bytes: the store as it is, at its offset.)
        if r.ty == 0 {
            return alloc::vec![(if addr == r.addr { String::new() } else { format!("+{}", addr.wrapping_sub(r.addr)) }, None)];
        }
        let off = addr.max(r.addr) - r.addr;
        let len = size.min(r.size.saturating_sub(off));
        self.tables.types.leaves(r.ty, off, len).into_iter().map(|(path, leaf, off, bits)| (path, Some((leaf, off, bits)))).collect()
    }

    /// Leaf `leaf` of the object at `base` (see members) as it was after
    /// record `at`, printed.
    fn value(&self, base: u32, (leaf, off, bits): Leaf, at: usize) -> String {
        let (b0, bn) = bits.unwrap_or((0, 0));
        let mut v = String::new();
        self.tables.types.print(&self.rec.memory_at(self.now, at), base + off, leaf, b0, bn, &mut v);
        v
    }

    /// Variable set `i` (a Var record)'s value, printed.
    fn var_value(&self, i: usize) -> String {
        let Record::Var { var, value, .. } = self.rec.records[i] else { return String::new() };
        let v = self.rec.calls.func_at(i).and_then(|f| self.tables.vars.get(&(f, var)));
        v.and_then(|v| self.var_object(v, value, self.now)?.value).unwrap_or_default()
    }

    /// Write `w` of `named`, printed: each member it wrote, from what to what.
    fn changes(&self, named: &Named, w: &Wrote) -> Vec<Change> {
        let r = &named.regions[w.region as usize];
        if r.kind == "var" {
            let before = w.prev.map_or_else(|| "(uninitialized)".into(), |p| self.var_value(p));
            return alloc::vec![Change { member: String::new(), before, after: self.var_value(w.i) }];
        }
        self.members(r, w.i).into_iter().map(|(member, leaf)| {
            let (before, after) = match leaf {
                Some(leaf) => (self.value(r.addr, leaf, w.i.saturating_sub(1)), self.value(r.addr, leaf, w.i)),
                None => {
                    let Record::Store { old, .. } = self.rec.records[w.i] else { unreachable!() };
                    (old.map_or_else(|| "(not kept)".into(), num), num(self.wrote(w.i)))
                }
            };
            Change { member, before, after }
        }).collect()
    }

    /// The stores in records `first..=last` (a call's), and the variables
    /// set there, in order, each by what the code names that it hit (see
    /// Region): a call's pointer argument to a struct, a local, the task
    /// running, a callee's frame (from its frame base up to its caller's:
    /// the stack it used beyond its named locals; no type), a global of
    /// the kernel's; else by its address.
    pub fn stores(&self, first: usize, last: usize) -> alloc::rc::Rc<Named> {
        // (The last few spans asked: the call in focus's, and those the
        // pointer shows for a moment.)
        let mut cache = self.rec.named.borrow_mut();
        if let Some(k) = cache.iter().position(|(span, _)| *span == (first, last)) {
            let hit = cache.remove(k);
            cache.push(hit.clone());
            return hit.1;
        }
        drop(cache);
        let got = alloc::rc::Rc::new(self.name_stores(first, last));
        let mut cache = self.rec.named.borrow_mut();
        if cache.len() >= 4 {
            cache.remove(0);
        }
        cache.push(((first, last), got.clone()));
        got
    }

    /// The stores in records `first..=last` (see stores), each with what it
    /// hit and the member.
    pub fn stored(&self, first: usize, last: usize) -> Vec<Stored> {
        let named = self.stores(first, last);
        named.writes.iter().map(|w| {
            let r = &named.regions[w.region as usize];
            let member = self.members(r, w.i).into_iter().map(|(m, _)| m).collect::<Vec<_>>().join(", ");
            Stored { i: w.i, what: r.what.clone(), kind: r.kind, member }
        }).collect()
    }

    /// What each line of `file` stored (or set) in records `first..=last`
    /// by record `cur` (the source's notes): each thing it wrote (a
    /// member, a variable), its last change and how many times, said short
    /// (the member, or the variable, and the change: the line says the
    /// rest) and in full; at the line's last write.
    pub fn notes(&self, first: usize, last: usize, file: &str, cur: usize) -> Vec<Note> {
        let named = self.stores(first, last);
        let files: Vec<u32> = (0..self.tables.files.len() as u32).filter(|&k| self.tables.files[k as usize] == file).collect();
        // (Each line's things written, in the order first written: (region,
        // member) -> its last write, how many; and the line's last write.)
        let mut by: BTreeMap<u32, (Vec<((u32, String), (&Wrote, usize))>, usize)> = BTreeMap::new();
        for w in named.writes.iter().take_while(|w| w.i <= cur) {
            let Some(at) = w.at.filter(|at| files.contains(&at.loc.file)) else { continue };
            let r = &named.regions[w.region as usize];
            let members = if r.kind == "var" { alloc::vec![String::new()] } else { self.members(r, w.i).into_iter().map(|(m, _)| m).collect() };
            let (things, i) = by.entry(at.loc.line).or_default();
            *i = w.i;
            for m in members {
                let key = (w.region, m);
                match things.iter_mut().find(|(k, _)| *k == key) {
                    Some((_, x)) => *x = (w, x.1 + 1),
                    None => things.push((key, (w, 1))),
                }
            }
        }
        by.into_iter().map(|(line, (things, i))| {
            // (Short, the same said once, the times added: two inlined
            // helpers' `regs` set alike.)
            let (mut short, mut full): (Vec<(String, usize)>, Vec<String>) = (Vec::new(), Vec::new());
            for ((region, member), (w, n)) in things {
                let r = &named.regions[region as usize];
                let Some(x) = self.changes(&named, w).into_iter().find(|x| x.member == member) else { continue };
                let join = if x.member.starts_with('+') { "" } else if r.kind == "object" { "->" } else { "." };
                // (The member after the name, the function it is in last: "filp->f_pos in vfs_read".)
                let (expr, within) = r.what.split_once(" in ").map_or((r.what.as_str(), String::new()), |(e, f)| (e, format!(" in {f}")));
                let times = |n: usize| if n > 1 { format!(" ×{n}") } else { String::new() };
                full.push(format!("{expr}{}{within}: {} → {}{}", if x.member.is_empty() { String::new() } else { format!("{join}{}", x.member) }, x.before, x.after, times(n)));
                let name = if x.member.is_empty() { expr.rsplit(|c: char| !(c.is_alphanumeric() || c == '_')).next().unwrap_or(expr) } else { &x.member };
                let said = format!("{name}: {} → {}", x.before, x.after);
                match short.iter_mut().find(|(s, _)| *s == said) {
                    Some((_, k)) => *k += n,
                    None => short.push((said, n)),
                }
            }
            let short = short.into_iter().map(|(s, n)| if n > 1 { format!("{s} ×{n}") } else { s }).collect::<Vec<_>>().join("   ");
            Note { line, i, short, full: full.join("; ") }
        }).collect()
    }

    /// (The work of stores: see it.)
    fn name_stores(&self, first: usize, last: usize) -> Named {
        let (t, types) = (self.tables, &self.tables.types);
        // (The objects: what (as the code names it: "file in vfs_read"),
        // type, address, size, and for a call's local or frame its life (the
        // call's records: its slots are other calls' before and after). The
        // task first.)
        let task = self.rec.tasks.get(first).copied().unwrap_or(0);
        let mut regions: Vec<Region> = Vec::new();
        let mut seen = alloc::collections::BTreeSet::new();
        // (Named at record `by`: by the call there, its depth.)
        let mut add = |what: String, kind: &'static str, ty: u32, addr: u32, size: Option<u32>, call: Option<usize>, by: Option<usize>| {
            let ty = types.resolve(ty);
            if addr != 0 && seen.insert((addr, ty, call)) {
                let life = call.map(|c| (c, self.rec.calls.call_end(c).unwrap_or(usize::MAX)));
                let depth = by.and_then(|i| self.rec.calls.call_at(i)).map_or(0, |c| c.depth);
                regions.push(Region { what, kind, ty, addr, size: size.unwrap_or_else(|| types.size_of(ty)), life, depth });
            }
        };
        if let Some(ty) = types.find(arch::TASK) {
            // (By its name, "prog(19)": the task running as the span begins,
            // which another may follow in it; not "current".)
            let who = self.task(task);
            let who = format!("{}({})", who.comm.unwrap_or_default(), who.pid.unwrap_or(0));
            add(who.clone(), "task", ty, task, None, None, None);
            // (And what its pointer members point to, as the span ends:
            // current->files, ->fs, ->mm: "files->next_fd" through a local
            // the optimizer kept in a register is reached this way.)
            let end = self.rec.memory_at(self.now, last.min(last_record(self.rec)));
            if let Kind::Struct(members) = &types.get(types.resolve(ty)).kind {
                for mb in members.iter().filter(|mb| mb.bit_size == 0) {
                    if let Some(to) = types.struct_target(mb.ty) {
                        add(format!("{who}->{}", mb.name), "object", to, end.read_u32(task.wrapping_add(mb.bit_offset / 8)), None, None, None);
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
        let last = last.min(last_record(self.rec));
        for i in first..=last {
            let Record::Block { func, n: 0 } = records[i] else { continue };
            let Some((f, (words, frame))) = t.functions.get(&func).zip(args.get(i)) else { continue };
            // (What the machine's own functions name, its: the call's plumbing.)
            let machine = t.file(f.loc).starts_with(arch::MACHINE);
            let kind = |k: &'static str| if machine { "machine" } else { k };
            // (Its frame: up to its caller's, if the caller is known.)
            let caller = calls.call_from(i).and_then(|c| c.parent).and_then(|p| args.get(calls.calls[p as usize].first));
            if let Some((_, caller)) = caller && caller > frame {
                add(format!("frame of {}", f.name), "frame", 0, frame, Some(caller - frame), Some(i), Some(i));
            }
            for ((pname, ty), v) in types.params.get(&func).into_iter().flatten().zip(t.values(func, words)) {
                let v = v as u32;
                if let Some(to) = types.struct_target(*ty) {
                    add(format!("{pname} in {}", f.name), kind("object"), to, v, None, frame_of(i, v), Some(i));
                }
            }
            // (Where its pointers point as the call ends.)
            let end = calls.call_end(i).unwrap_or(last).min(last);
            for (var, ty, off) in types.locals.get(&func).into_iter().flatten().filter_map(|(v, t, o)| Some((v, t, (*o)?))) {
                add(format!("{var} in {}", f.name), kind("local"), *ty, frame.wrapping_add(off), None, Some(i), Some(i));
                // (A local pointing at a struct: that too ("files->next_fd"
                // through `struct files_struct *files = current->files`).)
                if let Some(to) = types.struct_target(*ty) {
                    let v = self.rec.memory_at(self.now, end).read_u32(frame.wrapping_add(off));
                    add(format!("{var} in {}", f.name), kind("object"), to, v, None, frame_of(i, v), Some(i));
                }
            }
            // (One the code keeps in a wasm local, pointing at a struct
            // (Record::Var): its last value; in the order the code first
            // sets them: the call's own filp names the file before an
            // inlined callee's file.)
            let mut values: Vec<(&Var, u32)> = Vec::new();
            for j in calls.vars_of(i, end) {
                if let Record::Var { var, value, .. } = records[j] && let Some(v) = t.vars.get(&(func, var)) {
                    match values.iter_mut().find(|(w, _)| w.function == v.function && w.name == v.name) {
                        Some(w) => w.1 = value as u32,
                        None => values.push((v, value as u32)),
                    }
                }
            }
            for (v, value) in values {
                if let Some(to) = self.var_ty(v).and_then(|ty| types.struct_target(ty)) {
                    add(format!("{} in {}", v.name, v.function), kind("object"), to, value, None, frame_of(i, value), Some(i));
                }
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
        // (Each store to the outermost region holding its first byte. The
        // regions by address: a store's are those starting at most the
        // largest one's size below it.)
        let mut by_start: Vec<usize> = (0..regions.len()).collect();
        by_start.sort_unstable_by_key(|&k| regions[k].addr);
        let widest = regions.iter().map(|r| r.size).max().unwrap_or(0).max(1);
        // (Regions of their own, once each: a global, an address nothing names.)
        let mut own: BTreeMap<u32, u32> = BTreeMap::new();
        let mut writes = Vec::new();
        for &i in self.rec.stores_between(first, last) {
            let Record::Store { addr, .. } = records[i] else { continue };
            let end = by_start.partition_point(|&k| regions[k].addr <= addr);
            // (The outermost: an object inside another (a page's refcount,
            // the atomic_t a callee names) is the other's member; a frame
            // only if nothing else holds it; of equal ones, the first named.)
            let hit = by_start[..end].iter().rev().take_while(|&&k| addr - regions[k].addr < widest).copied()
                .filter(|&k| addr < regions[k].addr.saturating_add(regions[k].size) && regions[k].life.is_none_or(|(a, b)| a <= i && i <= b))
                .min_by_key(|&k| (regions[k].kind == "frame", core::cmp::Reverse(regions[k].size), k));
            let region = match hit {
                Some(k) => k as u32,
                None => {
                    let r = match global_at(addr) {
                        Some((gaddr, size, name, ty)) => {
                            let machine = types.declared.get(name).is_some_and(|(f, _)| f.starts_with(arch::MACHINE));
                            Region { what: name.into(), kind: if machine { "machine" } else { "global" }, ty: types.resolve(ty), addr: gaddr, size, life: None, depth: u32::MAX }
                        }
                        None => Region { what: format!("{addr:#x}"), kind: "other", ty: 0, addr, size: 0, life: None, depth: u32::MAX },
                    };
                    *own.entry(r.addr).or_insert_with(|| {
                        regions.push(r);
                        regions.len() as u32 - 1
                    })
                }
            };
            writes.push(Wrote { i, region, at: self.rec.line_at(t, i), prev: None });
        }
        // (And each variable kept in a wasm local, as the code sets it: from
        // its value before, in its call, to its value then.)
        // (Each call's variable's region (none: of no type known), and its last set.)
        let mut vars: BTreeMap<(usize, u32), (Option<u32>, Option<usize>)> = BTreeMap::new();
        for i in first..=last {
            let Record::Var { var, .. } = records[i] else { continue };
            let Some(c) = calls.call_at(i) else { continue };
            let (region, prev) = vars.entry((c.first, var)).or_insert_with(|| {
                let v = t.vars.get(&(c.func, var));
                let r = v.and_then(|v| Some(Region { what: format!("{} in {}", v.name, v.function), kind: "var", ty: self.var_ty(v)?, addr: 0, size: 0, life: None, depth: c.depth }));
                (r.map(|r| (regions.push(r), regions.len() as u32 - 1).1), None)
            });
            let Some(region) = *region else { continue };
            writes.push(Wrote { i, region, at: self.rec.line_at(t, i), prev: prev.replace(i) });
        }
        writes.sort_by_key(|w| w.i);
        Named { regions, writes }
    }

    /// What records `first..=last` (a call's) did to the kernel's data:
    /// the objects they changed, not on a stack (a call's locals, and what
    /// a pointer reaches there, are the Stack's; nor the machine's own
    /// variables: the call's plumbing), each member from what it was to
    /// what it is as the span ends; those that end changed first, then
    /// those left as they were (Node::group); as a graph, by their pointers
    /// then. In the order of the calls that name them, the outer first
    /// (then the first written first).
    pub fn objects(&self, first: usize, last: usize) -> Vec<Node> {
        let types = &self.tables.types;
        let named = self.stores(first, last);
        // (Each object's writes (its region's, and any other's of its
        // address and type), by member, in order; with the member's leaf.)
        let mut nodes: Vec<(Node, Vec<(String, Leaf, Vec<Wrote>)>)> = Vec::new();
        let mut leaves: BTreeMap<(u32, u32, u32), Vec<(String, Option<Leaf>)>> = BTreeMap::new();
        for w in &named.writes {
            let r = &named.regions[w.region as usize];
            // (The kernel's data, not a stack's: a call's locals, and what a
            // pointer reaches on a stack, are the Stack's.)
            if !matches!(r.kind, "object" | "task" | "global") || r.size == 0 || r.life.is_some() {
                continue;
            }
            let group = "changed";
            let k = match nodes.iter().position(|(n, _)| n.addr == r.addr && n.id == r.ty) {
                Some(k) => k,
                None => {
                    nodes.push((Node { what: r.what.clone(), kind: r.kind, ty: types.name(r.ty), id: r.ty, addr: r.addr, size: r.size, changes: Vec::new(),
                                       depth: r.depth, group, under: None }, Vec::new()));
                    nodes.len() - 1
                }
            };
            // (A store's members (one may copy several), once per place.)
            let Record::Store { addr, size, .. } = self.rec.records[w.i] else { continue };
            let wrote = leaves.entry((w.region, addr, size)).or_insert_with(|| self.members(r, w.i)).clone();
            let members = &mut nodes[k].1;
            for (member, leaf) in wrote {
                match members.iter_mut().find(|(m, _, _)| *m == member) {
                    Some((_, _, ws)) => ws.push(*w),
                    None => {
                        let Some(leaf) = leaf else { continue };
                        members.push((member, leaf, alloc::vec![*w]));
                    }
                }
            }
        }
        // (Each member's changes; one only ever written as it was (memory
        // zeroed that was zero), not; an object with none, not. A pointer:
        // where it points as the span ends.)
        let mem = self.rec.memory_at(self.now, last.min(last_record(self.rec)));
        let mut nodes: Vec<(Node, Vec<Option<u32>>)> = nodes.into_iter().filter_map(|(mut n, members)| {
            let mut points = Vec::new();
            for (member, leaf, writes) in members {
                let before = |i: usize| self.value(n.addr, leaf, i.saturating_sub(1));
                let after = |i: usize| self.value(n.addr, leaf, i);
                // (A run: one function's writes in a row; one that left it
                // as it was, not.)
                let func = |w: &Wrote| w.at.and_then(|s| self.tables.functions.get(&s.func)).map(|f| f.name.as_str());
                let mut runs: Vec<Run> = Vec::new();
                for run in writes.chunk_by(|a, b| func(a) == func(b)) {
                    let (a, z) = (run[0], run[run.len() - 1]);
                    runs.push(Run { before: before(a.i), after: after(z.i), writes: run.len(), first: a.i, last: z.i, at: z.at.map(|s| self.line_only(&s)) });
                }
                let (a, z) = (runs[0].before.clone(), runs.last().expect("a run").after.clone());
                runs.retain(|r| r.before != r.after);
                if runs.is_empty() {
                    continue;
                }
                let (ty, off, _) = leaf;
                points.push(matches!(types.get(types.resolve(ty)).kind, Kind::Pointer(_)).then(|| mem.read_u32(n.addr.wrapping_add(off))));
                n.changes.push(Diff { member, before: a, after: z, to: None, runs });
            }
            if n.changes.is_empty() {
                return None;
            }
            // (Lasting, but every member as it was: its changes undone.)
            if n.changes.iter().all(|d| d.before == d.after) {
                n.group = "same";
            }
            // (The members it ends with changed first, then those as they were.)
            let mut both: Vec<(Diff, Option<u32>)> = n.changes.drain(..).zip(points).collect();
            both.sort_by_key(|(d, _)| d.before == d.after);
            let points;
            (n.changes, points) = both.into_iter().unzip();
            Some((n, points))
        }).collect();
        // (What changed, then what was left as it was; in each, in the order of the calls that name them, the outer first,
        // as a call tree reads; within one call, the first written first.
        // The task first; globals last.)
        nodes.sort_by_key(|n| (n.0.group == "same", n.0.depth));
        let (mut nodes, points): (Vec<Node>, Vec<Vec<Option<u32>>>) = nodes.into_iter().unzip();
        // (One name for two (the slabs of two caches a callee named by one
        // parameter): the later numbered: "slab in pfmemalloc_match #2".)
        let mut seen: BTreeMap<String, u32> = BTreeMap::new();
        for n in &mut nodes {
            let k = seen.entry(n.what.clone()).or_default();
            *k += 1;
            if *k > 1 {
                n.what = format!("{} #{k}", n.what);
            }
        }
        let within = |p: u32| nodes.iter().position(|n| n.addr <= p && p < n.addr.saturating_add(n.size));
        // (Each node's pointers to the others of its group, as memory is as
        // the span ends: the tree each group is shown as.)
        let links: Vec<Vec<(String, usize)>> = nodes.iter().enumerate().map(|(k, n)| {
            types.pointers(n.id, 3).into_iter().filter_map(|(member, off)| Some((member, within(mem.read_u32(n.addr.wrapping_add(off)))
                .filter(|&to| to != k && nodes[to].group == n.group)?))).collect()
        }).collect();
        let tos: Vec<Vec<Option<usize>>> = points.iter().enumerate().map(|(k, p)| p.iter().map(|p| p.and_then(within).filter(|&to| to != k)).collect()).collect();
        // (The tree they are shown in, from the ones nothing points to (then
        // any left: a cycle's), each under the first, in order, that points
        // to it.)
        let mut under = alloc::vec![None; nodes.len()];
        let mut placed = alloc::vec![false; nodes.len()];
        let pointed: alloc::collections::BTreeSet<usize> = links.iter().flatten().map(|l| l.1).collect();
        let roots = (0..nodes.len()).filter(|k| !pointed.contains(k)).chain(0..nodes.len());
        for root in roots {
            if placed[root] {
                continue;
            }
            placed[root] = true;
            let mut queue = alloc::collections::VecDeque::from([root]);
            while let Some(k) = queue.pop_front() {
                for (member, to) in &links[k] {
                    if !placed[*to] {
                        (placed[*to], under[*to]) = (true, Some((k, member.clone())));
                        queue.push_back(*to);
                    }
                }
            }
        }
        // (A pointer the call wrote: the node it points to.)
        for ((n, to), u) in nodes.iter_mut().zip(tos).zip(under) {
            for (d, to) in n.changes.iter_mut().zip(to) {
                d.to = to;
            }
            n.under = u;
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
        types.syscalls.values().find(|n| n.as_str() == name)?;
        let params = types.syscall_params(name).into_iter().flatten()
            .map(|(pname, ty)| Param { name: pname.clone(), ty: types.name(*ty), consts: consts_of(name, pname).iter().map(|s| s.to_string()).collect() }).collect();
        Some(SyscallInfo { params })
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
                // (A char *: and the string it points to, if it looks like one: "0x1d40130 → \"/sub\"", as any pointer's target.)
                let text = if p != 0 && self.is_string(*to) { c_string(mem, p) } else { None };
                o.value = Some(match text {
                    _ if p == 0 => "NULL".into(),
                    Some(t) => format!("{p:#x} → {t}"),
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
    let (lhs, rhs) = r.operands(signed)?;
    let v = |x: i128| if signed { x.to_string() } else { num(x as u64) };
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
    Some(crate::types::quote(&buf[..n]))
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

    /// Records `first..=last`'s writes, a member each: (record, what, kind, member, before, after).
    fn writes(v: &View, first: usize, last: usize) -> Vec<(usize, String, &'static str, String, String, String)> {
        let n = v.stores(first, last);
        n.writes.iter().flat_map(|w| {
            let r = &n.regions[w.region as usize];
            v.changes(&n, w).into_iter().map(|c| (w.i, r.what.clone(), r.kind, c.member, c.before, c.after))
        }).collect()
    }

    #[test]
    fn a_variable_in_a_wasm_local_has_its_last_value() {
        // f (1) keeps int x (variable 0, a local: no offset) in a wasm
        // local: it takes 5, then 7.
        let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\nV\t1\t0\t1\tf\tx\t0\t0\n", "T\t1\tbase\tint\t4\ts\nQ\t1\tx:1:\n");
        let r = [Record::Block { func: 1, n: 0 }, Record::Var { var: 0, line: 0, value: 5 }, Record::Block { func: 1, n: 1 }, Record::Var { var: 0, line: 0, value: 7 }];
        let mut args = Args::default();
        args.push(0, &[64]);
        let rec = Recording::new(&t, r.to_vec(), (0..4).collect(), args);
        let mem = Mem(alloc::vec![0; 128]);
        let v = View { tables: &t, rec: &rec, now: &mem };
        let x = |i| v.call_stack(i, None, &mem, i)[0].locals.clone().unwrap_or_default().into_iter().find(|o| o.name == "x").and_then(|o| o.value);
        assert_eq!((x(0), x(2), x(3)), (Some("(uninitialized)".into()), Some("5".into()), Some("7".into())));
    }

    #[test]
    fn each_task_where_it_last_ran() {
        // Task 0x100 runs f (1), switches (__switch_to, 2) to 0x200, which
        // runs f.
        let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\nF\t2\t__switch_to\t0:1\t3\n", "");
        let r = [Record::Block { func: 1, n: 0 }, Record::Block { func: 2, n: 0 }, Record::Return { func: 2, value: 0 }, Record::Block { func: 1, n: 0 }];
        let mut args = Args::default();
        args.push(1, &[0x100, 0x200, 0]);
        let rec = Recording::new(&t, r.to_vec(), (0..4).collect(), args);
        let v = View { tables: &t, rec: &rec, now: &Mem(alloc::vec![0; 16]) };
        let at = |i| v.tasks(i).into_iter().map(|x| (x.task.addr, x.running, x.first, x.last)).collect::<Vec<_>>();
        assert_eq!(at(0), [(0x100, true, 0, Some(0)), (0x200, false, 2, None)]);
        assert_eq!(at(3), [(0x100, false, 0, Some(1)), (0x200, true, 2, Some(3))]);
        assert_eq!(v.tasks(3)[0].func.as_deref(), Some("f"));
    }

    #[test]
    fn a_store_is_a_change_to_its_member() {
        // struct s { int a; int b; } at 4 (f's argument p): record 1 stores
        // b (2, was 1), record 3 a byte of a (5): each a change to its member.
        let t = Tables::parse("U\nP\t0\tf.c\nF\t9\tf\t0:1\t0\n",
                              "T\t1\tbase\tint\t4\ts\nT\t2\tstruct\ts\t8\nM\t2\ta\t0\t0\t1\nM\t2\tb\t32\t0\t1\nT\t3\tptr\t\t4\t2\nP\t9\tf\tp:3\n");
        let r = [Record::Block { func: 9, n: 0 }, Record::Store { addr: 8, size: 4, old: Some(1), line: 0, from: None },
                 Record::Block { func: 9, n: 1 }, Record::Store { addr: 5, size: 1, old: Some(0), line: 0, from: None }];
        let mut args = Args::default();
        args.push(0, &[4, 64]);
        let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 4], args);
        let mut m = alloc::vec![0u8; 16];
        (m[8], m[5]) = (2, 5);
        let v = View { tables: &t, rec: &rec, now: &Mem(m) };
        let got: Vec<_> = writes(&v, 0, 3).into_iter().map(|(i, _, _, member, before, after)| (i, member, before, after)).collect();
        assert_eq!(got, [(1, "b".into(), "1".into(), "2".into()), (3, "a".into(), "0".into(), "1280".into())]);
    }

    #[test]
    fn one_store_copying_a_struct_changes_each_member() {
        // f (1) takes struct path { void *mnt; void *dentry; } *p (at 8) and
        // copies one into it with one 8-byte store, on line 3 (its block's
        // second): mnt 0 → 5, dentry 0 → 6, in the Objects and the line's note.
        let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\nL\t1\t0\t0:1;:3\n",
                              "T\t1\tptr\t\t4\t0\nT\t2\tstruct\tpath\t8\nM\t2\tmnt\t0\t0\t1\nM\t2\tdentry\t32\t0\t1\nT\t3\tptr\t\t4\t2\nP\t1\tf\tp:3\n");
        let r = [Record::Block { func: 1, n: 0 }, Record::Store { addr: 8, size: 8, old: Some(0), line: 1, from: None }];
        let mut args = Args::default();
        args.push(0, &[8, 64]);
        let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 2], args);
        let mut m = alloc::vec![0u8; 32];
        (m[8], m[12]) = (5, 6);
        let v = View { tables: &t, rec: &rec, now: &Mem(m) };
        let nodes = v.objects(0, 1);
        let got: Vec<_> = nodes[0].changes.iter().map(|d| (d.member.clone(), d.after.clone())).collect();
        assert_eq!(got, [("mnt".into(), "0x5".into()), ("dentry".into(), "0x6".into())]);
        let notes = v.notes(0, 1, "f.c", 1);
        assert_eq!((notes[0].line, notes[0].short.as_str()), (3, "mnt: NULL → 0x5   dentry: NULL → 0x6"));
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
                 Record::Store { addr: 8, size: 4, old: Some(1), line: 0, from: None }, Record::Store { addr: 36, size: 4, old: Some(0), line: 0, from: None },
                 Record::Block { func: 2, n: 0 },
                 Record::Store { addr: 20, size: 1, old: Some(0), line: 0, from: None }, Record::Return { func: 2, value: 0 },
                 Record::Store { addr: 100, size: 1, old: Some(0), line: 0, from: None }, Record::Store { addr: 21, size: 1, old: Some(0), line: 0, from: None }];
        let mut args = Args::default();
        args.push(0, &[4, 32]);
        args.push(3, &[16]);
        let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 8], args);
        let mut m = alloc::vec![0u8; 128];
        (m[8], m[36], m[20], m[100]) = (2, 7, 9, 5);
        let v = View { tables: &t, rec: &rec, now: &Mem(m) };
        // (The last, to g's frame after g returned: g's no more.)
        let got: Vec<_> = writes(&v, 0, 7).into_iter().map(|(_, what, kind, member, _, after)| (what, kind, member, after)).collect();
        assert_eq!(got, [("p in f".into(), "object", "b".into(), "2".into()), ("x in f".into(), "local", "".into(), "7".into()),
                         ("frame of g".into(), "frame", "+4".into(), "9".into()), ("0x64".into(), "other", "".into(), "5".into()),
                         ("0x15".into(), "other", "".into(), "0".into())]);
    }

    #[test]
    fn a_store_through_a_pointer_in_a_wasm_local_is_named_by_it() {
        // f (1) keeps struct s *q (variable 0) in a wasm local, 4 (a set,
        // itself one of the call's stores); it stores q->b.
        let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\nV\t1\t0\t1\tf\tq\t0\t0\n",
                              "T\t1\tbase\tint\t4\ts\nT\t2\tstruct\ts\t8\nM\t2\ta\t0\t0\t1\nM\t2\tb\t32\t0\t1\nT\t3\tptr\t\t4\t2\nQ\t1\tq:3:\n");
        let r = [Record::Block { func: 1, n: 0 }, Record::Var { var: 0, line: 0, value: 4 }, Record::Store { addr: 8, size: 4, old: Some(1), line: 0, from: None }];
        let mut args = Args::default();
        args.push(0, &[64]);
        let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 3], args);
        let mut m = alloc::vec![0u8; 128];
        m[8] = 2;
        let v = View { tables: &t, rec: &rec, now: &Mem(m) };
        let got: Vec<_> = writes(&v, 0, 2).into_iter().map(|(_, what, _, member, _, after)| (what, member, after)).collect();
        assert_eq!(got, [("q in f".into(), "".into(), "0x4".into()), ("q in f".into(), "b".into(), "2".into())]);
    }

    #[test]
    fn the_objects_a_call_wrote_are_a_graph() {
        // f (1) takes struct s *p (at 4) and struct t *q (at 32: int c, then
        // struct s *link): it stores p->b, q->c, and q->link, pointing at
        // *p: two nodes, p under q, by its link.
        let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\n",
                              "T\t1\tbase\tint\t4\ts\nT\t2\tstruct\ts\t8\nM\t2\ta\t0\t0\t1\nM\t2\tb\t32\t0\t1\nT\t3\tptr\t\t4\t2\n\
                               T\t4\tstruct\tt\t8\nM\t4\tc\t0\t0\t1\nM\t4\tlink\t32\t0\t3\nT\t5\tptr\t\t4\t4\nP\t1\tf\tp:3,q:5\n");
        let st = |addr, old| Record::Store { addr, size: 4, old: Some(old), line: 0, from: None };
        let r = [Record::Block { func: 1, n: 0 }, st(8, 1), st(32, 0), st(36, 0)];
        let mut args = Args::default();
        args.push(0, &[4, 32, 64]);
        let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 4], args);
        let mut m = alloc::vec![0u8; 64];
        (m[8], m[32], m[36]) = (2, 7, 4);
        let v = View { tables: &t, rec: &rec, now: &Mem(m) };
        let nodes = v.objects(0, 3);
        let got: Vec<_> = nodes.iter().map(|n| (n.what.clone(), n.changes.iter().map(|d| (d.member.clone(), format!("{} → {}", d.before, d.after), d.to)).collect::<Vec<_>>())).collect();
        assert_eq!(got, [("p in f".into(), alloc::vec![("b".into(), "1 → 2".into(), None)]),
                         ("q in f".into(), alloc::vec![("c".into(), "0 → 7".into(), None), ("link".into(), "NULL → 0x4".into(), Some(0))])]);
        // (Shown as a tree: p under q, which points to it.)
        assert_eq!(nodes.iter().map(|n| n.under.clone()).collect::<Vec<_>>(), [Some((1, "link".into())), None]);
    }

    #[test]
    fn a_calls_locals_are_not_objects() {
        // f (1) stores its local int x (at frame 32 + 4): the Stack's, not
        // an object, before it returns as after.
        let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\n", "T\t1\tbase\tint\t4\ts\nQ\t1\tx:1:4\n");
        let r = [Record::Block { func: 1, n: 0 }, Record::Store { addr: 36, size: 4, old: Some(0), line: 0, from: None }, Record::Return { func: 1, value: 0 }];
        let mut args = Args::default();
        args.push(0, &[32]);
        let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 3], args);
        let mut m = alloc::vec![0u8; 64];
        m[36] = 7;
        let v = View { tables: &t, rec: &rec, now: &Mem(m) };
        assert!(v.objects(0, 2).is_empty() && v.objects(0, 1).is_empty());
    }

    #[test]
    fn an_object_inside_another_is_its_member() {
        // f (1) takes struct u *p (at 4: int a, then struct s inner); g
        // (2), which f calls, takes struct s *q (p's inner, at 8) and stores
        // q->b: one object, p in f, its inner.b changed.
        let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\nF\t2\tg\t0:9\t0\n",
                              "T\t1\tbase\tint\t4\ts\nT\t2\tstruct\ts\t8\nM\t2\ta\t0\t0\t1\nM\t2\tb\t32\t0\t1\nT\t3\tptr\t\t4\t2\n\
                               T\t4\tstruct\tu\t12\nM\t4\ta\t0\t0\t1\nM\t4\tinner\t32\t0\t2\nT\t5\tptr\t\t4\t4\nP\t1\tf\tp:5\nP\t2\tg\tq:3\n");
        let r = [Record::Block { func: 1, n: 0 }, Record::Block { func: 2, n: 0 }, Record::Store { addr: 12, size: 4, old: Some(0), line: 0, from: None }];
        let mut args = Args::default();
        args.push(0, &[4, 64]);
        args.push(1, &[8, 48]);
        let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 3], args);
        let mut m = alloc::vec![0u8; 128];
        m[12] = 3;
        let v = View { tables: &t, rec: &rec, now: &Mem(m) };
        let got: Vec<_> = v.objects(0, 2).into_iter().map(|n| (n.what, n.changes.into_iter().map(|d| (d.member, format!("{} → {}", d.before, d.after))).collect::<Vec<_>>())).collect();
        assert_eq!(got, [("p in f".into(), alloc::vec![("inner.b".into(), "0 → 3".into())])]);
    }

    #[test]
    fn two_objects_of_one_name_are_numbered() {
        // f (1), called twice, takes struct s *p (at 4, then at 20) and
        // stores p->b: two objects, "p in f" and "p in f #2".
        let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\n",
                              "T\t1\tbase\tint\t4\ts\nT\t2\tstruct\ts\t8\nM\t2\ta\t0\t0\t1\nM\t2\tb\t32\t0\t1\nT\t3\tptr\t\t4\t2\nP\t1\tf\tp:3\n");
        let st = |addr| Record::Store { addr, size: 4, old: Some(0), line: 0, from: None };
        let r = [Record::Block { func: 1, n: 0 }, st(8), Record::Return { func: 1, value: 0 }, Record::Block { func: 1, n: 0 }, st(24), Record::Return { func: 1, value: 0 }];
        let mut args = Args::default();
        args.push(0, &[4, 64]);
        args.push(3, &[20, 64]);
        let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 6], args);
        let mut m = alloc::vec![0u8; 128];
        (m[8], m[24]) = (1, 2);
        let v = View { tables: &t, rec: &rec, now: &Mem(m) };
        let got: Vec<_> = v.objects(0, 5).into_iter().map(|n| n.what).collect();
        assert_eq!(got, ["p in f", "p in f #2"]);
    }

    #[test]
    fn a_members_changes_are_a_run_a_function() {
        // f (1) takes struct s *p (at 4) and stores p->b 2 (was 1); g (2),
        // which it calls with p, stores it back to 1: b left as it was, f's
        // run 1 → 2, then g's 2 → 1.
        let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\nF\t2\tg\t0:5\t0\nL\t1\t0\t0:1\nL\t2\t0\t0:5\n",
                              "T\t1\tbase\tint\t4\ts\nT\t2\tstruct\ts\t8\nM\t2\ta\t0\t0\t1\nM\t2\tb\t32\t0\t1\nT\t3\tptr\t\t4\t2\nP\t1\tf\tp:3\nP\t2\tg\tp:3\n");
        let st = |old| Record::Store { addr: 8, size: 4, old: Some(old), line: 0, from: None };
        let r = [Record::Block { func: 1, n: 0 }, st(1), Record::Block { func: 2, n: 0 }, st(2), Record::Return { func: 2, value: 0 }];
        let mut args = Args::default();
        args.push(0, &[4, 64]);
        args.push(2, &[4, 48]);
        let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 5], args);
        let mut m = alloc::vec![0u8; 128];
        m[8] = 1;
        let v = View { tables: &t, rec: &rec, now: &Mem(m) };
        let nodes = v.objects(0, 4);
        let d = &nodes[0].changes[0];
        let runs: Vec<_> = d.runs.iter().map(|r| (r.at.as_ref().map(|l| l.func.clone()), r.before.clone(), r.after.clone(), r.last)).collect();
        assert_eq!((nodes[0].what.as_str(), d.member.as_str(), d.before.as_str(), d.after.as_str()), ("p in f", "b", "1", "1"));
        // (Left as it was, in the end: an object of the "same" group.)
        assert_eq!(nodes[0].group, "same");
        assert_eq!(runs, [(Some("f".into()), "1".into(), "2".into(), 1), (Some("g".into()), "2".into(), "1".into(), 3)]);
    }

    #[test]
    fn a_member_one_function_writes_back_is_no_change() {
        // f (1) takes struct s *p (at 4): it stores p->b 2 (was 1), then 1
        // again: b left as it was, by the one function: no change, no object.
        let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\n",
                              "T\t1\tbase\tint\t4\ts\nT\t2\tstruct\ts\t8\nM\t2\ta\t0\t0\t1\nM\t2\tb\t32\t0\t1\nT\t3\tptr\t\t4\t2\nP\t1\tf\tp:3\n");
        let st = |old| Record::Store { addr: 8, size: 4, old: Some(old), line: 0, from: None };
        let r = [Record::Block { func: 1, n: 0 }, st(1), st(2)];
        let mut args = Args::default();
        args.push(0, &[4, 32]);
        let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 3], args);
        let mut m = alloc::vec![0u8; 64];
        m[8] = 1;
        let v = View { tables: &t, rec: &rec, now: &Mem(m) };
        assert!(v.objects(0, 2).is_empty());
    }

    #[test]
    fn a_value_is_followed_through_its_copies() {
        // 7 stored at 0x30 (computed), then copied to 0x10 (a load, a store),
        // then to 0x20 by a memcpy of 8 (two stores): 0x24's way back is
        // 0x24 <- 0x14 <- ... 0x14 was never written; 0x20's is 0x20 <- 0x10 <- 0x30.
        let t = Tables::default();
        let st = |addr, from| Record::Store { addr, size: 4, old: Some(0), line: 0, from: core::num::NonZeroU32::new(from) };
        let r = [st(0x30, 0), st(0x10, 0x30), st(0x20, 0x10), st(0x24, 0x14)];
        let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 4], Args::default());
        let v = View { tables: &t, rec: &rec, now: &Mem(alloc::vec![0; 64]) };
        let way = |a| v.origin(a, 4, 3, 8).iter().map(|w| (w.i, w.addr, w.from)).collect::<Vec<_>>();
        assert_eq!(way(0x20), [(2, 0x20, Some(0x10)), (1, 0x10, Some(0x30)), (0, 0x30, None)]);
        assert_eq!(way(0x24), [(3, 0x24, Some(0x14))]);
    }

    #[test]
    fn frames_are_where_their_calls_are() {
        // f (1) runs lines 1 and 2, calls g (2) at 2, then runs 3.
        let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\nF\t2\tg\t0:9\t0\n\
                               L\t1\t0\t0:1;:2;|;:3\nL\t2\t0\t0:10\n", "");
        let r = [Record::Block { func: 1, n: 0 }, Record::Block { func: 2, n: 0 }];
        let rec = Recording::new(&t, r.to_vec(), alloc::vec![0, 1], Args::default());
        let v = View { tables: &t, rec: &rec, now: &Mem(Vec::new()) };
        let lines: Vec<_> = v.call_stack(1, None, &Mem(Vec::new()), 1).iter().map(|f| (f.func.clone(), f.line)).collect();
        assert_eq!(lines, [("g".into(), Some(10)), ("f".into(), Some(2))]);
    }

    #[test]
    fn inlined_code_is_a_frame_of_its_own() {
        // f (1) runs line 1, then h's line 20 (h inlined into f at line 2),
        // which calls g (2).
        let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\nF\t2\tg\t0:9\t0\n\
                               I\t0\th\t0:2\nL\t1\t0\t0:1;:20@0;|\nL\t2\t0\t0:10\n", "");
        let r = [Record::Block { func: 1, n: 0 }, Record::Block { func: 2, n: 0 }];
        let rec = Recording::new(&t, r.to_vec(), alloc::vec![0, 1], Args::default());
        let v = View { tables: &t, rec: &rec, now: &Mem(Vec::new()) };
        let frames: Vec<_> = v.call_stack(1, None, &Mem(Vec::new()), 1).iter().map(|f| (f.func.clone(), f.line, f.inlined)).collect();
        assert_eq!(frames, [("g".into(), Some(10), false), ("h".into(), Some(20), true), ("f".into(), Some(2), false)]);
        // (Line 2, where h is inlined, ran too: h's line 20 ran for it.)
        assert!(v.coverage("f.c", None).iter().any(|l| l.line == 2));
        // At record 0, its first line (nth 0): f at line 1, nothing inlined.
        let first: Vec<_> = v.call_stack(0, Some(0), &Mem(Vec::new()), 0).iter().map(|f| (f.func.clone(), f.line)).collect();
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
        assert_eq!((value(0).as_str(), value(4).as_str()), ("0x8 → \"tmp\"", "0x10"));
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
