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


mod frames;
mod objects;
mod stores;
mod value;
#[cfg(test)]
mod tests;

pub use frames::{run_to, step, Called, Frame, Line};
pub use objects::{Diff, Group, Node, Run};
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

impl Task {
    /// As the page names it: comm(pid).
    pub fn named(&self) -> String {
        format!("{}({})", self.comm.as_deref().unwrap_or_default(), self.pid.unwrap_or(0))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SyscallSummary {
    pub task: Task,
    /// Its name (its number, if this kernel's table has none: see
    /// Types::syscall_name).
    pub name: String,
    /// The result (None: still running at the end of the recording).
    pub ret: Option<i32>,
    /// Its span in the records (still running: to the last record).
    pub first: usize,
    pub last: usize,
}


/// The chart window made by a wheel gesture. Its ends are inclusive, while
/// the returned span is the distance between them. A gesture follows its
/// dominant axis so trackpad cross-axis noise cannot pan and zoom at once.
/// `width` is positive; `zoom` is the vertical gesture's span multiplier.
pub fn wheel_window((a, b): (usize, usize), x: f64, width: f64, dx: f64, dy: f64, shift: bool, zoom: f64) -> (f64, usize) {
    let span = (b - a + 1) as f64;
    if shift || dx.abs() > dy.abs() {
        let delta = if shift { dy } else { dx };
        return (a as f64 + delta / width * span, b - a);
    }

    let at = (x / width).clamp(0.0, 1.0);
    let new = span * zoom;
    (a as f64 + at * (span - new), (new as usize).saturating_sub(1))
}

/// A drag of `dx` px over a chart `width` px wide, its window `window`, kept
/// in the recording's `steps`: the window's start and span then, and the
/// offset (px) the drawn chart moves by meanwhile.
pub fn clamped_pan((a, b): (usize, usize), dx: f64, width: f64, steps: usize) -> (f64, usize, f64) {
    let span = b.saturating_sub(a);
    let visible = span.saturating_add(1).min(steps.max(1));
    let max_start = steps.saturating_sub(visible) as f64;
    let start = (a as f64 - dx / width.max(1.0) * visible as f64).clamp(0.0, max_start);
    (start, span, (a as f64 - start) / visible as f64 * width.max(1.0))
}

/// A stack chart (View::chart): its columns, and the bars they stack.
#[derive(Debug, Default)]
pub struct Chart {
    pub bars: Vec<Bar>,
    pub cols: Vec<Column>,
}

impl Chart {
    /// Each row's runs: a bar over the columns it is on, in a row, as the
    /// page draws it: (depth, first column, one past the last, bar).
    pub fn bar_runs(&self) -> Vec<(u32, usize, usize, u32)> {
        let n = self.cols.len();
        let depth = self.cols.iter().map(|c| c.frames.len()).max().unwrap_or(0);
        let mut runs = Vec::new();
        for dd in 0..depth {
            let mut k = 0;
            while k < n {
                let Some(&bar) = self.cols.get(k).and_then(|c| c.frames.get(dd)) else { k += 1; continue };
                let mut e = k + 1;
                while self.cols.get(e).and_then(|c| c.frames.get(dd)) == Some(&bar) {
                    e += 1;
                }
                runs.push((dd as u32, k, e, bar));
                k = e;
            }
        }
        runs
    }

    /// Which task ran when: a run of columns each, as the page draws it
    /// under the chart: (first column, one past the last, task).
    pub fn task_runs(&self) -> Vec<(usize, usize, u32)> {
        let mut runs: Vec<(usize, usize, u32)> = Vec::new();
        for (k, c) in self.cols.iter().enumerate() {
            match runs.last_mut() {
                Some(r) if r.2 == c.task => r.1 = k + 1,
                _ => runs.push((k, k + 1, c.task)),
            }
        }
        runs
    }
}

/// A column: its step and record, the task running there, and its stack
/// then, as bars (indexes in Chart::bars), outermost first.
#[derive(Debug)]
pub struct Column {
    pub step: usize,
    pub i: usize,
    pub task: u32,
    pub frames: Vec<u32>,
}

/// A bar: a call from record `first`, or code inlined into it; its
/// function's name (the inlined one's), and the call's file.
#[derive(Debug)]
pub struct Bar {
    pub first: usize,
    pub name: String,
    pub file: Option<String>,
    pub inlined: bool,
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
    pub fn task(&self, addr: u32, at: usize) -> Task {
        let t = self.tables;
        let at = at.min(last_record(self.rec));
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
                doing: last.and_then(|j| {
                    let syscall = self.rec.syscall_at(j).and_then(|s| t.types.syscalls.get(&self.rec.syscalls[s].nr).cloned());
                    syscall.or_else(|| self.rec.calls.stack_at(t, j, None).into_iter().next().and_then(|c| t.functions.get(&c.func)).map(|f| f.name.clone()))
                }),
            }
        }).collect()
    }

    /// Syscall `k` (Recording::syscalls).
    pub fn syscall(&self, k: usize) -> Option<SyscallSummary> {
        let s = self.rec.syscalls.get(k)?;
        Some(SyscallSummary {
            task: self.task(s.task, s.first),
            name: self.tables.types.syscall_name(s.nr),
            ret: s.ret,
            first: s.first,
            last: s.last.unwrap_or(last_record(self.rec)),
        })
    }

    pub fn syscalls(&self) -> Vec<SyscallSummary> {
        (0..self.rec.syscalls.len()).filter_map(|k| self.syscall(k)).collect()
    }

    /// A stack chart of steps `first..=last` in `n` columns, each a
    /// moment spread evenly over the steps (Recording::steps): the task
    /// there and its call stack then, outermost first, each call and the
    /// code inlined into it a bar (in `bars`, once each).
    pub fn chart(&self, first: usize, last: usize, n: usize) -> Chart {
        let (t, rec) = (self.tables, self.rec);
        let mut chart = Chart::default();
        let last = last.min(rec.steps.len().saturating_sub(1));
        if rec.steps.is_empty() || n == 0 || first > last {
            return chart;
        }
        let mut known: BTreeMap<(usize, Option<u32>), u32> = BTreeMap::new();
        // (The k-th column: step first + k * span / n.)
        let span = last - first + 1;
        for step in (0..n).map(|k| first + k * span / n) {
            let (i, nth, cut) = rec.steps[step];
            let mut frames = Vec::new();
            let stack = rec.calls.stack_at(t, i, Some(nth as usize));
            for (d, c) in stack.iter().enumerate() {
                // (The innermost call without the code inlined its line has not entered yet, as the Stack has it.)
                let mut nest = self.nest(c);
                if d + 1 == stack.len() {
                    nest.drain(..(cut as usize).min(nest.len() - 1));
                }
                for (k, _) in nest.into_iter().rev() {
                    let bar = *known.entry((c.first, k)).or_insert_with(|| {
                        let place = self.place(c.func);
                        let name = k.map_or_else(|| place.as_ref().map_or("?".into(), |p| p.name.clone()), |k| self.inline_name(k));
                        chart.bars.push(Bar { first: c.first, name, file: place.map(|p| p.file), inlined: k.is_some() });
                        chart.bars.len() as u32 - 1
                    });
                    frames.push(bar);
                }
            }
            chart.cols.push(Column { step, i, task: rec.tasks.get(i).copied().unwrap_or(0), frames });
        }
        chart
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

    /// The lines of `file` that code ran, in syscall `n` (or the whole
    /// recording), with how often (the most of the blocks running it): the
    /// lines each block runs (Tables::runs: inlined code's, and where it is
    /// inlined, too).
    pub fn coverage(&self, file: &str, n: Option<usize>) -> BTreeMap<u32, u64> {
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
        lines
    }
}

/// A comparison, its operands and whether it held: "18 >= 0 ✓" (signed
/// ones as such, else as numbers are).
fn compared(r: &Record) -> Option<String> {
    let (lhs, op, rhs, signed, held) = r.comparison()?;
    let v = |x: i128| if signed { x.to_string() } else { num(x as u64) };
    Some(format!("{} {op} {} {}", v(lhs), v(rhs), if held { "✓" } else { "✗" }))
}

/// The last record's index (0 if none).
fn last_record(rec: &Recording) -> usize {
    rec.records.len().saturating_sub(1)
}
