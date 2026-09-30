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

mod frames;
mod objects;
mod stores;
mod value;
#[cfg(test)]
mod tests;

pub use frames::{Called, Frame, Line};
pub use objects::{Diff, Group, Node, Run, Under};
pub use stores::{Named, Note, Region, RegionKind, Wrote, Write};
pub use value::{Missing, Object, Target};

/// Who a task was, at a record (see View::task).
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

/// A record's kind, as the page names it ("store"): a block 0 with its
/// arguments is a call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize), serde(rename_all = "lowercase"))]
pub enum RecordKind {
    Call,
    Block,
    Return,
    Store,
    Compare,
    Var,
    Task,
}

/// Where the kernel was at record `i`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct At {
    pub i: usize,
    /// Its record's kind (a block 0's: Block).
    pub kind: RecordKind,
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

/// A task's fields: the key ones first (arch::TASK_FIELDS, in that
/// order), then the others.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct TaskFields {
    pub fields: Vec<Object>,
}

/// A task at a record: what the scheduler has it doing (`state`); its
/// `last` record by then (None: it first runs after, or the recording
/// never saw it run), and there the syscall it is in, else its
/// outermost call (what it does: off the CPU, its innermost is always the
/// switch).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct TaskAt {
    pub task: TaskRef,
    /// The task it sits under (its task_struct's address): a thread, its
    /// thread group's leader; a process, its parent.
    pub up: Option<u32>,
    pub state: TaskState,
    /// Its last record by then, if the recording has one.
    pub last: Option<usize>,
    pub syscall: Option<String>,
    pub func: Option<String>,
}

/// What the scheduler has a task doing then.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize), serde(rename_all = "lowercase"))]
pub enum TaskState {
    /// On the CPU.
    Running,
    /// On the run queue, waiting for it.
    Runnable,
    /// Off it, waiting for something.
    Sleeping,
    /// The kernel's task list no longer has it.
    Exited,
    /// Not made yet.
    Later,
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

    /// Who the task at `addr` was at record `at`: its pid and comm as memory
    /// was then (a task renamed later, or one whose task_struct another has
    /// by now, as it was).
    pub fn task(&self, addr: u32, at: usize) -> TaskRef {
        let t = self.tables;
        let at = at.min(last_record(self.rec));
        // (Read once per snapshot and per last write to its pid or comm by
        // then: every store names its task.)
        let h = self.rec.stores();
        let last = |f: &str| t.types.field(arch::TASK, f).and_then(|(off, len)| h.last_write(&self.rec.records, addr.wrapping_add(off), len, at)).map(|(i, _)| i);
        let key = (addr, last(arch::TASK_PID).max(last(arch::TASK_COMM)));
        let known = self.rec.names.borrow().get(&key).cloned();
        let task = known.or_else(|| {
            let task = read_task(t, &self.rec.memory_at(self.now, at), addr)?;
            self.rec.names.borrow_mut().insert(key, task.clone());
            Some(task)
        });
        TaskRef { addr, pid: task.as_ref().map(|t| t.pid), comm: task.map(|t| t.comm) }
    }

    /// The fields of the task at `addr`, in `mem` (memory as it was): the
    /// key ones first (arch::TASK_FIELDS), then the others as declared.
    pub fn task_fields(&self, mem: &dyn Memory, addr: u32) -> TaskFields {
        let types = &self.tables.types;
        let mut fields = types.find(arch::TASK).and_then(|ty| types.object(mem, arch::CURRENT, addr, ty, 1).children).unwrap_or_default();
        fields.sort_by_key(|o| arch::TASK_FIELDS.iter().position(|&k| k == o.name).unwrap_or(usize::MAX));
        TaskFields { fields }
    }

    /// The tasks at record `i`: the kernel's, as its lists had them then
    /// (arch::INIT_TASK), then the others the recording has (exited by
    /// then, or made later) (TaskAt).
    pub fn tasks(&self, i: usize) -> Vec<TaskAt> {
        let t = self.tables;
        let mem = self.rec.memory_at(self.now, i);
        let field = |addr: u32, s: &str, f: &str| t.types.field(s, f).map(|(off, _)| mem.read_u32(addr.wrapping_add(off)));
        let offset = |s: &str, f: &str| t.types.field(s, f).map(|(off, _)| off);
        // (The kernel's: each process, then its threads, the leader among them.)
        let mut list: Vec<u32> = Vec::new();
        if let (Some(&(init, _)), Some(next), Some(node), Some(head)) = (t.types.globals.get(arch::INIT_TASK), offset(arch::TASK, arch::TASK_LIST),
                                                                         offset(arch::TASK, arch::THREAD_NODE), offset(arch::TASK_SIGNAL.1, arch::THREAD_HEAD)) {
            let mut p = init;
            // (Guards: a list read from memory as it was may be torn.)
            while list.len() < 4096 {
                let threads = field(p, arch::TASK, arch::TASK_SIGNAL.0).unwrap_or(0).wrapping_add(head);
                let mut n = mem.read_u32(threads);
                while n != threads && n != 0 && list.len() < 4096 {
                    list.push(n.wrapping_sub(node));
                    n = mem.read_u32(n);
                }
                p = mem.read_u32(p.wrapping_add(next)).wrapping_sub(next);
                if p == init || p == next.wrapping_neg() {
                    break;
                }
            }
        }
        // (Each task the recording has, in the order it first runs: its last record by `i`.)
        let mut seen: Vec<(u32, Option<usize>)> = Vec::new();
        for (j, &task) in self.rec.tasks.iter().enumerate().filter(|&(_, &task)| task != 0) {
            let k = match seen.iter().position(|s| s.0 == task) {
                Some(k) => k,
                None => (seen.push((task, None)), seen.len() - 1).1,
            };
            if j <= i {
                seen[k].1 = Some(j);
            }
        }
        // (With no list (the tables have none), every task counts as on it.)
        let known = !list.is_empty();
        let others: Vec<u32> = seen.iter().map(|s| s.0).filter(|a| !list.contains(a)).collect();
        let running = self.rec.tasks.get(i).copied();
        list.iter().map(|&a| (a, true)).chain(others.into_iter().map(|a| (a, !known))).map(|(addr, listed)| {
            let last = seen.iter().find(|s| s.0 == addr).and_then(|s| s.1);
            let on_rq = t.types.field(arch::TASK, arch::TASK_ON_RQ).is_some_and(|(off, _)| {
                let mut b = [0u8];
                mem.read(addr.wrapping_add(off), &mut b);
                b[0] != 0
            });
            let state = match () {
                _ if running == Some(addr) => TaskState::Running,
                _ if !listed && last.is_none() => TaskState::Later,
                _ if !listed => TaskState::Exited,
                _ if on_rq => TaskState::Runnable,
                _ => TaskState::Sleeping,
            };
            let up = |f: &str| field(addr, arch::TASK, f).filter(|&p| p != 0 && p != addr);
            TaskAt {
                task: self.task(addr, i),
                up: up(arch::TASK_LEADER).or_else(|| up(arch::TASK_PARENT)),
                state, last,
                syscall: last.and_then(|j| self.rec.syscall_at(j)).and_then(|s| t.types.syscalls.get(&self.rec.syscalls[s].nr).cloned()),
                func: last.and_then(|j| self.rec.calls.stack_at(t, j, None).into_iter().next()).and_then(|c| t.functions.get(&c.func)).map(|f| f.name.clone()),
            }
        }).collect()
    }

    pub fn syscalls(&self) -> Vec<SyscallSummary> {
        self.rec.syscalls.iter().enumerate().map(|(index, s)| SyscallSummary {
            index,
            id: self.rec.ids.get(s.first).copied().unwrap_or(0),
            task: self.task(s.task, s.first),
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
                (RecordKind::Block, func, (b.map(|b| b.loc()), b.and_then(|b| b.steps.first()?.inlined)))
            }
            Record::Return { func, .. } => {
                let prev = (0..i).rev().find(|&j| tasks[j] == tasks[i]);
                (RecordKind::Return, func, located(func, prev.and_then(top).and_then(|(_, s)| s)))
            }
            // (A store, a comparison: on its line, the record's, see line_at.)
            r @ (Record::Store { .. } | Record::Compare { .. }) => {
                let kind = if matches!(r, Record::Store { .. }) { RecordKind::Store } else { RecordKind::Compare };
                match self.rec.line_at(t, i) {
                    Some(l) => (kind, l.func, (Some(l.loc), l.inlined)),
                    None => (kind, 0, (None, None)),
                }
            }
            r @ (Record::Task { .. } | Record::Var { .. }) => {
                let (func, step) = top(i).unwrap_or((0, None));
                let kind = if matches!(r, Record::Task { .. }) { RecordKind::Task } else { RecordKind::Var };
                (kind, func, located(func, step))
            }
        };
        Some(At {
            i, kind,
            func: t.functions.get(&func).map_or(String::new(), |f| f.name.clone()),
            file: loc.map(|l| t.file(l).to_string()),
            line: loc.map(|l| l.line),
            inlined: self.inlined_from(inlined),
            task: self.task(self.rec.tasks[i], i),
            syscall: self.rec.syscall_at(i),
            value: match records[i] { Record::Return { value, .. } => Some(value), _ => None },
        })
    }

    /// Records `from..=to` as text, a line each, as the recording has them
    /// (the raw trace): "call ksys_read(fd=3, buf=0x1cefed0, count=64)
    /// fs/read_write.c:710 frame=0x1ce000" (the block 0 and the Args record
    /// after it, as one line), "block ksys_read #3 fs/read_write.c:712",
    /// "store 0x1ce0020 [8] 0 → 26", "return ksys_read = 26"; a task switch
    /// as a line of its own. Each with its kind (RecordKind).
    pub fn raw(&self, from: usize, to: usize) -> Vec<(usize, RecordKind, String)> {
        let (t, rec) = (self.tables, self.rec);
        let mut out = Vec::new();
        for i in from..=to.min(last_record(rec)) {
            if i > 0 && rec.tasks.get(i) != rec.tasks.get(i - 1) {
                let task = self.task(rec.tasks[i], i);
                out.push((i, RecordKind::Task, format!("── {}({}) runs", task.comm.unwrap_or_default(), task.pid.unwrap_or(0))));
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
                        (RecordKind::Call, format!("call {}({}){loc} frame={frame:#x}", self.name(func), each.collect::<Vec<_>>().join(", ")))
                    } else {
                        (RecordKind::Block, format!("block {} #{n}{loc}", self.name(func)))
                    }
                }
                Record::Return { func, value } => (RecordKind::Return, format!("return {} = {}", self.name(func), value)),
                Record::Task { .. } => continue, // (the task's line is above)
                // (As its type says: a pointer in hex, a string's text, an enumerator.)
                Record::Var { var, value, .. } => {
                    let v = self.var_at(i, var);
                    let shown = v.and_then(|v| self.var_object(v, value, self.now)).and_then(|o| o.value).unwrap_or_else(|| num(value));
                    (RecordKind::Var, format!("var {} = {shown}", v.map_or("?", |v| v.name.as_str())))
                }
                r @ Record::Compare { .. } => (RecordKind::Compare, format!("compare {}", compared(&r).unwrap_or_default())),
                Record::Store { addr, size, old, from, .. } => {
                    let from = from.map_or(String::new(), |f| format!(" ← {f:#x}"));
                    (RecordKind::Store, match old {
                        None => format!("store {addr:#x} [{size} bytes] (old contents not kept){from}"),
                        Some(old) => format!("store {addr:#x} [{size}] {} → {}{from}", num(old), num(self.wrote(i))),
                    })
                }
            };
            out.push((i, kind, text));
        }
        out
    }

    /// The task running at the start of each of `n` buckets of the recording.
    pub fn timeline(&self, n: usize) -> Vec<TaskRef> {
        let steps = self.rec.steps().max(1);
        (0..n).filter_map(|b| {
            let i = self.rec.record_of(b * steps / n);
            Some(self.task(*self.rec.tasks.get(i)?, i))
        }).collect()
    }

    /// Where `name` is defined: an instrumented function (its source), else
    /// a function with no probe (inline), a type, a global, or a member as
    /// "struct.member" (".member": of any struct) (where it is declared).
    pub fn definition(&self, name: &str) -> Option<Place> {
        if let Some(id) = self.tables.find(name) {
            return self.place(id);
        }
        let (file, line) = self.tables.types.declared.get(name)?;
        Some(Place { name: name.to_string(), file: file.clone(), line: *line })
    }

    /// Where member `member` of type `ty` (a struct or union, or a pointer
    /// to one) is declared; with no type, of the one struct that has a
    /// member so named, if only one does.
    pub fn member_definition(&self, ty: Option<u32>, member: &str) -> Option<Place> {
        let types = &self.tables.types;
        let Some(ty) = ty else {
            let suffix = format!(".{member}");
            let mut of = types.declared.keys().filter(|k| k.ends_with(&suffix) && !k[..k.len() - suffix.len()].contains('.'));
            return match (of.next(), of.next()) {
                (Some(k), None) => self.definition(k),
                _ => None,
            };
        };
        let id = types.resolve(ty);
        let id = match types.get(id).kind {
            Kind::Pointer(to) => types.resolve(to),
            _ => id,
        };
        self.definition(&format!("{}.{member}", types.get(id).name))
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

/// The C string at `p`, up to its NUL, quoted as C writes it (what is not
/// printable escaped; past 64 characters, "…"), as a char * says there is.
/// (A __user one too: a program's memory is kernel memory.)
fn c_string(mem: &dyn Memory, p: u32) -> String {
    let mut buf = [0u8; 65];
    mem.read(p, &mut buf);
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    crate::types::quote(&buf[..n])
}
