//! Views of a recording, for the page: what it shows, by name rather than
//! by id.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use alloc::collections::BTreeMap;

use crate::arch;
use crate::recording::Recording;
use crate::stack::{self, LineStep};
use crate::tables::{Loc, Tables, Var};
use crate::trace::Record;
use crate::types::{c_str, num, quote, Kind, Types};
use crate::Memory;


mod chart;
mod frames;
mod objects;
mod stores;
mod value;
#[cfg(test)]
mod tests;

pub use chart::{clamped_pan, wheel_window, Bar, Chart, Column};
pub use frames::{run_to, step, Called, Frame, Line};
pub use objects::{Diff, Group, Node, Streak};
pub use stores::{Note, RegionKind, Write};
pub(crate) use stores::{Named, Wrote};
pub use value::{Missing, Object, Target};

/// Who a task was, at a record (see View::task).
#[derive(Debug, Clone, PartialEq)]
pub struct Task {
    pub addr: u32,
    pub pid: Option<i32>,
    pub comm: Option<String>,
}

impl RunSummary {
    /// As the page heads it: "read by prog(19)", "running by swapper/0(0)".
    pub fn title(&self) -> String {
        format!("{} by {}", self.name, self.task.named())
    }
}

impl Task {
    /// As the page names it: comm(pid).
    pub fn named(&self) -> String {
        format!("{}({})", self.comm.as_deref().unwrap_or_default(), self.pid.unwrap_or(0))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunSummary {
    pub task: Task,
    /// A syscall's number (none: a run between them).
    pub nr: Option<u32>,
    /// Its name (a syscall's, its number if this kernel's table has none:
    /// see Types::syscall_name; a run's, "running").
    pub name: String,
    /// The result (None: still running at the end of the recording).
    pub ret: Option<i32>,
    /// Its span in the records (still running: to the last record).
    pub first: usize,
    pub last: usize,
}


/// Where something is declared (a function, a type, a global, a member);
/// a function the recording has, to `last` (the last line of its blocks in
/// its own file), and whether only its call and returns are recorded
/// (tables::Function::sealed).
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub name: String,
    pub file: String,
    pub line: u32,
    pub last: Option<u32>,
    pub sealed: bool,
}


/// A task at a record: what the scheduler has it doing (`state`); its
/// `last` record by then (None: it first runs after, or the recording
/// never saw it run), and there what it is `doing`: the syscall it is in,
/// else its outermost call (off the CPU, its innermost is always the
/// switch).
#[derive(Debug, Clone, PartialEq)]
pub struct TaskAt {
    pub task: Task,
    /// The task it sits under (its task_struct's address): a thread, its
    /// thread group's leader; a process, its parent.
    pub up: Option<u32>,
    pub state: TaskState,
    /// Its last record by then, if the recording has one.
    pub last: Option<usize>,
    pub doing: Option<String>,
}

/// What the scheduler has a task doing then.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

impl View<'_> {
    fn name(&self, func: u32) -> String {
        self.tables.functions.get(&func).map_or_else(|| format!("{func:#x}"), |f| f.name.clone())
    }

    /// Who the task at `addr` was at record `at`: its pid and comm as memory
    /// was then (a task renamed later, or one whose task_struct another has
    /// by now, as it was).
    pub fn task(&self, addr: u32, at: usize) -> Task {
        let t = self.tables;
        let at = at.min(self.rec.last());
        // (Read once per snapshot and per last write to its pid or comm by
        // then: every store names its task.)
        let h = self.rec.stores();
        let last = |f: &str| t.types.field(arch::TASK, f).and_then(|(off, len)| h.last_write(addr.wrapping_add(off), len, at)).map(|(i, _)| i);
        let key = (addr, last(arch::TASK_PID).max(last(arch::TASK_COMM)));
        let known = self.rec.names.borrow().get(&key).cloned();
        let task = known.or_else(|| {
            let task = t.types.read_task(&self.rec.memory_at(self.now, at), addr)?;
            self.rec.names.borrow_mut().insert(key, task.clone());
            Some(task)
        });
        Task { addr, pid: task.as_ref().map(|t| t.pid), comm: task.map(|t| t.comm) }
    }

    /// The fields of the task at `addr`, as memory was after record `at`:
    /// the key ones first (arch::TASK_FIELDS), then the others as declared.
    pub fn task_fields(&self, addr: u32, at: usize) -> Vec<Object> {
        let types = &self.tables.types;
        let mem = self.rec.memory_at(self.now, at);
        let mut fields = types.find(arch::TASK).and_then(|ty| types.object(&mem, arch::CURRENT, addr, ty, 1).children).unwrap_or_default();
        fields.sort_by_key(|o| arch::TASK_FIELDS.iter().position(|&k| k == o.name).unwrap_or(usize::MAX));
        fields
    }

    /// The tasks at record `i`: the kernel's, as its lists had them then
    /// (arch::INIT_TASK), then the others the recording has (exited by
    /// then, or made later) (TaskAt).
    pub fn tasks(&self, i: usize) -> Vec<TaskAt> {
        let t = self.tables;
        let mem = self.rec.memory_at(self.now, i);
        // (The fields read, where they are: looked up once.)
        let offset = |s: &str, f: &str| t.types.field(s, f).map(|(off, _)| off);
        let of_task = |f: &str| offset(arch::TASK, f);
        let (signal, on_rq, leader, parent) = (of_task(arch::TASK_SIGNAL.0), of_task(arch::TASK_ON_RQ), of_task(arch::TASK_LEADER), of_task(arch::TASK_PARENT));
        let word = |addr: u32, off: Option<u32>| off.map(|off| mem.read_u32(addr.wrapping_add(off)));
        // (The kernel's: each process, then its threads, the leader among them.)
        let mut list: Vec<u32> = Vec::new();
        if let (Some(&(init, _)), Some(next), Some(node), Some(head)) = (t.types.globals.get(arch::INIT_TASK), offset(arch::TASK, arch::TASK_LIST),
                                                                         offset(arch::TASK, arch::THREAD_NODE), offset(arch::TASK_SIGNAL.1, arch::THREAD_HEAD)) {
            let mut p = init;
            // (Guards: a list read from memory as it was may be torn.)
            while list.len() < 4096 {
                let threads = word(p, signal).unwrap_or(0).wrapping_add(head);
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
            let on_rq = on_rq.is_some_and(|off| mem.read_u8(addr.wrapping_add(off)) != 0);
            let state = match () {
                _ if running == Some(addr) => TaskState::Running,
                _ if !listed && last.is_none() => TaskState::Later,
                _ if !listed => TaskState::Exited,
                _ if on_rq => TaskState::Runnable,
                _ => TaskState::Sleeping,
            };
            let up = |off| word(addr, off).filter(|&p| p != 0 && p != addr);
            TaskAt {
                task: self.task(addr, i),
                up: up(leader).or_else(|| up(parent)),
                state, last,
                doing: last.and_then(|j| {
                    let syscall = self.rec.run_at(j).and_then(|s| t.types.syscalls.get(&self.rec.runs[s].nr?).cloned());
                    syscall.or_else(|| self.rec.calls.stack_at(t, j, None).into_iter().next().and_then(|c| t.functions.get(&c.func)).map(|f| f.name.clone()))
                }),
            }
        }).collect()
    }

    /// Run `k` (Recording::runs).
    pub fn run(&self, k: usize) -> Option<RunSummary> {
        let s = self.rec.runs.get(k)?;
        Some(RunSummary {
            task: self.task(s.task, s.first),
            // (A run between syscalls: its task running, whatever it does
            // then; a stack known only from where the recording began says
            // no more.)
            name: s.nr.map_or_else(|| "running".into(), |nr| self.tables.types.syscall_name(nr)),
            ret: s.ret,
            first: s.first,
            last: s.last.unwrap_or(self.rec.last()),
            nr: s.nr,
        })
    }

    /// Where `name` is defined: an instrumented function (its source), else
    /// a function with no probe (inline), a type, a global, or a member as
    /// "struct.member" (".member": of any struct) (where it is declared).
    pub fn definition(&self, name: &str) -> Option<Place> {
        if let Some(id) = self.tables.find(name) {
            return self.place(id);
        }
        let (file, line) = self.tables.types.declared.get(name)?;
        Some(Place { name: name.to_string(), file: file.clone(), line: *line, last: None, sealed: false })
    }

    /// Where member `member` is declared: of the one struct that has a
    /// member so named, if only one does.
    pub fn member_definition(&self, member: &str) -> Option<Place> {
        let suffix = format!(".{member}");
        let mut of = self.tables.types.declared.keys().filter(|k| k.ends_with(&suffix) && !k[..k.len() - suffix.len()].contains('.'));
        match (of.next(), of.next()) {
            (Some(k), None) => self.definition(k),
            _ => None,
        }
    }

    /// Function `id`: where it is.
    pub(crate) fn place(&self, id: u32) -> Option<Place> {
        let t = self.tables;
        let f = t.functions.get(&id)?;
        let last = t.blocks_of(id).filter(|l| l.file == f.loc.file).map(|l| l.line).fold(f.loc.line, u32::max);
        Some(Place { name: f.name.clone(), file: t.file(f.loc).to_string(), line: f.loc.line, last: Some(last), sealed: f.sealed })
    }

    /// The lines of `file` that code ran, in run `n` (or the whole
    /// recording), with how often (the most of the blocks running it): the
    /// lines each block runs (Tables::block_lines: inlined code's, and where it is
    /// inlined, too).
    pub fn coverage(&self, file: &str, n: Option<usize>) -> BTreeMap<u32, u64> {
        let t = self.tables;
        let counts = self.rec.block_counts(n);
        let file = t.file_id(file);
        let mut lines: BTreeMap<u32, u64> = BTreeMap::new();
        for (&(f, b), &c) in counts.iter().filter(|(_, c)| **c > 0) {
            for loc in t.block_lines(f, b).filter(|l| Some(l.file) == file) {
                let e = lines.entry(loc.line).or_default();
                *e = (*e).max(c);
            }
        }
        lines
    }
}


