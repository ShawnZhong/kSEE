//! A recording: the records of the trace ring, and what the host rebuilds
//! from them alone: the task running at each record, who the tasks were,
//! the syscalls, and the stores (memory as it was).

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::OnceCell;

use crate::stack::{self, LineStep};
use crate::syscalls::{self, Syscall};
use crate::tables::Tables;
use crate::task::Task;
use crate::trace::Record;
use crate::{mem, Memory};

#[derive(Debug, Default)]
pub struct Recording {
    pub records: Vec<Record>,
    /// Each record's id (trace::ring): it names the record in later
    /// recordings of the same ring too, where its index has moved on.
    pub ids: Vec<u32>,
    /// The task_struct address running at each record.
    pub tasks: Vec<u32>,
    /// In the order they began.
    pub syscalls: Vec<Syscall>,
    /// Who was at a task_struct address, from Task records: address ->
    /// [(record, task)]; and where tasks exited (do_exit): address ->
    /// [record]. The address of an exited task may be another's later.
    idents: BTreeMap<u32, Vec<(usize, Task)>>,
    exits: BTreeMap<u32, Vec<usize>>,
    /// The stores, indexed (once asked for).
    stores: OnceCell<mem::History>,
}

/// A program's run: from its Task record, until its task_struct is
/// another's (or the recording ends).
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub task: u32,
    pub first: usize,
    pub last: usize,
    /// Its Line records: (record, the program's source line), before each
    /// syscall.
    pub lines: Vec<(usize, u32)>,
}

impl Recording {
    pub fn new(t: &Tables, records: Vec<Record>, ids: Vec<u32>) -> Recording {
        // The task running at each record: it changes when __switch_to(prev,
        // next) returns (in next), and when a new task starts
        // (wasm_task_entry(p)); a Task record says so too.
        let (switch, start, do_exit) = (t.find("__switch_to"), t.find(stack::TASK_ENTRY), t.find("do_exit"));
        let args = |i: usize| match records.get(i + 1) {
            Some(&Record::Args { args, .. }) => Some(args),
            _ => None,
        };
        let (mut task, mut next, mut first_prev) = (0, None, None);
        let mut tasks = Vec::with_capacity(records.len());
        let mut idents: BTreeMap<u32, Vec<(usize, Task)>> = BTreeMap::new();
        let mut exits: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
        for (i, r) in records.iter().enumerate() {
            match *r {
                Record::Block { func, n: 0 } if Some(func) == switch => {
                    first_prev = first_prev.or(args(i).map(|a| (i, a[0])));
                    next = args(i).map(|a| a[1]);
                }
                Record::Block { func, n: 0 } if Some(func) == start => {
                    task = args(i).map_or(task, |a| a[0]);
                    next = None;
                }
                Record::Block { func, n: 0 } if Some(func) == do_exit => exits.entry(task).or_default().push(i),
                Record::Return { func, .. } if Some(func) == switch => task = next.take().unwrap_or(task),
                Record::Task { task: addr, pid, comm } => {
                    task = addr;
                    let bytes: Vec<u8> = comm.iter().flat_map(|w| w.to_le_bytes()).take_while(|&b| b != 0).collect();
                    let who = Task { pid: pid as i32, comm: String::from_utf8_lossy(&bytes).into() };
                    idents.entry(addr).or_default().push((i, who));
                }
                _ => {}
            }
            tasks.push(task);
        }
        // (Before the first switch, the task it switches from ran: until
        // then, the records say no more.)
        if let Some((at, prev)) = first_prev {
            for t in tasks[..at].iter_mut().take_while(|t| **t == 0) {
                *t = prev;
            }
        }
        let syscalls = syscalls::split(t.find(syscalls::ENTRY).unwrap_or(0), &records, &tasks);
        Recording { records, ids, tasks, syscalls, idents, exits, stores: OnceCell::new() }
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// The stores, indexed for memory as it was.
    pub fn stores(&self) -> &mem::History {
        self.stores.get_or_init(|| mem::History::new(&self.records))
    }

    /// Memory as it was after record `at`: the later stores undone from
    /// `end` (memory as it was when the recording ended).
    pub fn memory_at<'a>(&'a self, end: &'a dyn Memory, at: usize) -> mem::PastMemory<'a> {
        mem::PastMemory { base: end, history: self.stores(), at }
    }

    /// The task at `addr` at record `i`, as a Task record of its program
    /// says (before or after `i`, as long as it did not exit in between).
    pub fn task_at(&self, addr: u32, i: usize) -> Option<Task> {
        let exited = |a: usize, b: usize| self.exits.get(&addr).is_some_and(|e| e.iter().any(|&x| a <= x && x < b));
        let ids = self.idents.get(&addr)?;
        if let Some((at, t)) = ids.iter().rev().find(|(at, _)| *at <= i) {
            if !exited(*at, i) {
                return Some(t.clone());
            }
        }
        match ids.iter().find(|(at, _)| *at > i) {
            Some((at, t)) if !exited(i, *at) => Some(t.clone()),
            _ => None,
        }
    }

    /// The syscall in progress on the task running at record `i` (its index).
    pub fn syscall_at(&self, i: usize) -> Option<usize> {
        let task = *self.tasks.get(i)?;
        self.syscalls.iter().position(|s| s.task == task && s.first <= i && s.last.is_none_or(|l| i <= l))
    }

    /// The position of the next (dir > 0) or previous block record from `i`
    /// that starts a new `unit`: "block", "line" (another source line, on
    /// the task running at `i`), or "function" (another function than at
    /// `i`). `i` if none.
    pub fn seek(&self, t: &Tables, i: usize, dir: i32, unit: &str) -> usize {
        let func_at = |j: usize| match self.records.get(j) {
            Some(Record::Block { func, .. }) => Some(*func),
            _ => None,
        };
        let line_at = |j: usize| match self.records.get(j) {
            Some(&Record::Block { func, n }) => t.blocks.get(&(func, n)).map(|l| (l.file, l.line)),
            _ => None,
        };
        let (here, here_line, task) = (func_at(i), line_at(i), self.tasks.get(i).copied());
        let mut j = i;
        loop {
            if dir > 0 {
                if j + 1 >= self.len() {
                    return i;
                }
                j += 1;
            } else {
                if j == 0 {
                    return i;
                }
                j -= 1;
            }
            let Some(f) = func_at(j) else { continue };
            let new = match unit {
                "function" => Some(f) != here,
                "line" => self.tasks.get(j).copied() == task && line_at(j).is_some() && line_at(j) != here_line,
                _ => true,
            };
            if new {
                return j;
            }
        }
    }

    /// The latest run of program `pid` (a Task record says which).
    pub fn program(&self, pid: i32) -> Option<Run> {
        let (task, first) = self.idents.iter()
            .flat_map(|(&a, ids)| ids.iter().filter(|(_, t)| t.pid == pid).map(move |(i, _)| (a, *i)))
            .max_by_key(|&(_, i)| i)?;
        let until = self.idents[&task].iter().map(|(i, _)| *i).find(|&i| i > first).unwrap_or(self.len());
        let mut run = Run { task, first, last: first, lines: Vec::new() };
        for j in first..until {
            let line = match self.records[j] {
                Record::Line { line } => Some(line),
                _ => None,
            };
            if self.tasks[j] != task && line.is_none() {
                continue;
            }
            run.last = j;
            run.lines.extend(line.map(|l| (j, l)));
        }
        Some(run)
    }

    /// The first call of function `name` in records `first..=last` (of the
    /// task at address `task`, or any): its record.
    pub fn first_call(&self, t: &Tables, name: &str, first: usize, last: usize, task: Option<u32>) -> Option<usize> {
        let ids: Vec<u32> = t.functions.iter().filter(|(_, f)| f.name == name).map(|(&id, _)| id).collect();
        (first..=last.min(self.len().checked_sub(1)?)).find(|&i| {
            matches!(self.records[i], Record::Block { func, n: 0 } if ids.contains(&func))
                && task.is_none_or(|t| self.tasks[i] == t)
        })
    }

    /// How often each block ran in syscall `n` (or the whole recording).
    pub fn block_counts(&self, n: Option<usize>) -> BTreeMap<(u32, u32), u64> {
        let (first, last, task) = match n.and_then(|n| self.syscalls.get(n)) {
            Some(s) => (s.first, s.last.unwrap_or(usize::MAX), Some(s.task)),
            None if n.is_some() => return BTreeMap::new(),
            None => (0, usize::MAX, None),
        };
        let mut counts = BTreeMap::new();
        for (i, r) in self.records.iter().enumerate().skip(first).take(last.saturating_sub(first).saturating_add(1)) {
            if let Record::Block { func, n } = *r {
                if task.is_none_or(|t| self.tasks[i] == t) {
                    *counts.entry((func, n)).or_default() += 1;
                }
            }
        }
        counts
    }

    /// The source lines run in records `first..=last` of the task at `task`.
    pub fn lines(&self, t: &Tables, first: usize, last: usize, task: u32) -> Vec<LineStep> {
        stack::lines(t, &self.records, &self.tasks, first, last, Some(task))
    }

    /// The newest store to `addr..addr + len` at or before record `at` (or
    /// the end): (its record, the value it overwrote, the line that made it:
    /// where the function running then was).
    pub fn last_write(&self, t: &Tables, addr: u32, len: u32, at: usize) -> Option<(usize, u64, Option<LineStep>)> {
        let at = at.min(self.len().checked_sub(1)?);
        let (i, old) = self.stores().last_write(&self.records, addr, len, at)?;
        // (After a return into the middle of a block, the caller's line.)
        let line = stack::lines(t, &self.records, &self.tasks, i.saturating_sub(4000), i, self.tasks.get(i).copied()).pop();
        Some((i, old, line))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(func: u32, n: u32) -> Record {
        Record::Block { func, n }
    }

    fn task(addr: u32, pid: u32) -> Record {
        Record::Task { task: addr, pid, comm: [u32::from_le_bytes(*b"hi\0\0"), 0, 0, 0] }
    }

    #[test]
    fn tasks_switch_where_switch_to_returns() {
        let t = Tables::parse("F\t1\t__switch_to\t0:1\t3\n", "");
        let switch = t.find("__switch_to").unwrap();
        let r = [task(0x100, 5), b(switch, 0), Record::Args { args: [0x100, 0x200, 0, 0, 0, 0], n: 2 },
                 Record::Return { func: switch, value: 0 }, b(9, 1)];
        let rec = Recording::new(&t, r.to_vec(), (0..5).collect());
        assert_eq!(rec.tasks, [0x100, 0x100, 0x100, 0x200, 0x200]);
        assert_eq!(rec.task_at(0x100, 4).map(|t| (t.pid, t.comm)), Some((5, "hi".into())));
        assert_eq!(rec.task_at(0x200, 4), None);
    }

    #[test]
    fn a_program_runs_until_its_task_is_another() {
        let t = Tables::default();
        let r = [task(0x100, 5), Record::Line { line: 3 }, b(9, 0), task(0x100, 6), b(9, 1)];
        let rec = Recording::new(&t, r.to_vec(), (0..5).collect());
        assert_eq!(rec.program(5), Some(Run { task: 0x100, first: 0, last: 2, lines: alloc::vec![(1, 3)] }));
        assert_eq!(rec.program(6).map(|p| (p.first, p.last)), Some((3, 4)));
    }
}
