//! A recording: the records of the trace ring, and what the host rebuilds
//! from them alone: the task running at each record, the syscalls, and the
//! stores (memory as it was).

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::cell::OnceCell;

use crate::stack::{self, LineStep};
use crate::syscalls::{self, Syscall};
use crate::tables::Tables;
use crate::trace::{Args, Record};
use crate::{arch, mem, Memory};

#[derive(Debug, Default)]
pub struct Recording {
    pub records: Vec<Record>,
    /// Each record's id (trace::ring): it names the record in later
    /// recordings of the same ring too, where its index has moved on.
    pub ids: Vec<u32>,
    /// The calls' arguments, by their first record.
    pub args: Args,
    /// The task_struct address running at each record.
    pub tasks: Vec<u32>,
    /// In the order they began.
    pub syscalls: Vec<Syscall>,
    /// The steps: the block records (their indexes), in order. A step is
    /// the recording's unit of time (the timeline's axis): one basic block
    /// run; stores take none (a memset is not longer than a line).
    pub blocks: Vec<usize>,
    /// The calls, and where each record's task is in them.
    pub calls: stack::Calls,
    /// Who each task is (by its task_struct's address), read once per
    /// snapshot (View::task).
    pub names: core::cell::RefCell<alloc::collections::BTreeMap<u32, crate::task::Task>>,
    /// The last span's stores, named (View::stores: what Objects and the
    /// source's notes ask again at each moment), by its first and last.
    pub named: core::cell::RefCell<Option<((usize, usize), alloc::rc::Rc<crate::view::Named>)>>,
    /// The stores, indexed (once asked for).
    stores: OnceCell<mem::History>,
}

impl Recording {
    pub fn new(t: &Tables, records: Vec<Record>, ids: Vec<u32>, args: Args) -> Recording {
        // The task running at each record: it changes when __switch_to(prev,
        // next) returns (in next), when a new task starts
        // (wasm_task_entry(p)), and where the runtime says which runs (a
        // Task record, where a recording begins).
        let (switch, start) = (t.find(arch::SWITCH), t.find(arch::TASK_ENTRY));
        let arg = |i: usize, k: usize| args.get(i).and_then(|(a, _)| a.get(k).copied());
        let (mut task, mut next, mut first_prev) = (0, None, None);
        let mut tasks = Vec::with_capacity(records.len());
        for (i, r) in records.iter().enumerate() {
            match *r {
                Record::Block { func, n: 0 } if Some(func) == switch => {
                    first_prev = first_prev.or(arg(i, 0));
                    next = arg(i, 1);
                }
                Record::Block { func, n: 0 } if Some(func) == start => {
                    task = arg(i, 0).unwrap_or(task);
                    next = None;
                }
                Record::Return { func, .. } if Some(func) == switch => task = next.take().unwrap_or(task),
                Record::Task { task: t } => {
                    task = t;
                    next = None;
                }
                _ => {}
            }
            tasks.push(task);
        }
        // (Until the first switch, the task it switches from ran: until
        // then, the records say no more.)
        if let Some(prev) = first_prev {
            for t in tasks.iter_mut().take_while(|t| **t == 0) {
                *t = prev;
            }
        }
        let calls = stack::Calls::new(t, &records, &tasks);
        let syscalls = syscalls::split(t.find(arch::SYSCALL).unwrap_or(0), &records, &tasks, &args, &calls);
        let blocks: Vec<usize> = records.iter().enumerate().filter(|(_, r)| matches!(r, Record::Block { .. })).map(|(i, _)| i).collect();
        Recording { records, ids, args, tasks, syscalls, blocks, calls, stores: OnceCell::new(), names: Default::default(), named: Default::default() }
    }

    /// How many steps (blocks) the recording has.
    pub fn steps(&self) -> usize {
        self.blocks.len()
    }

    /// Record `i`'s step: the blocks up to it (its own included), so
    /// records between two blocks are at the earlier one's step.
    pub fn step_of(&self, i: usize) -> usize {
        self.blocks.partition_point(|&b| b <= i).saturating_sub(1)
    }

    /// The record step `k` begins at (its block), the last record if past
    /// the end.
    pub fn record_of(&self, k: usize) -> usize {
        self.blocks.get(k).copied().unwrap_or(self.records.len().saturating_sub(1))
    }

    /// The records `n` columns of steps `from..=to` (records) begin at, for
    /// a chart: the k-th column's, at step from + k * span / n.
    pub fn columns(&self, from: usize, to: usize, n: usize) -> Vec<usize> {
        let (a, b) = (self.step_of(from), self.step_of(to));
        let span = b - a + 1;
        (0..n).map(|k| self.record_of(a + k * span / n).max(from)).collect()
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

    /// The syscall in progress on the task running at record `i` (its index).
    pub fn syscall_at(&self, i: usize) -> Option<usize> {
        let task = *self.tasks.get(i)?;
        self.syscalls.iter().position(|s| s.task == task && s.first <= i && s.last.is_none_or(|l| i <= l))
    }

    /// The next (dir > 0) or previous block record from `i` whose block
    /// runs line `line` of `file` (its own code or code inlined into it).
    pub fn run_of(&self, t: &Tables, file: &str, line: u32, i: usize, dir: i32) -> Option<usize> {
        let files: Vec<u32> = (0..t.files.len() as u32).filter(|&k| t.files[k as usize] == file).collect();
        let at = |l: &crate::tables::Loc| l.line == line && files.contains(&l.file);
        // (The blocks that run it.)
        // (Its own code, or code inlined at it: Tables::runs.)
        let blocks: alloc::collections::BTreeSet<(u32, u32)> = t.blocks.keys()
            .filter(|&&(f, n)| t.runs(f, n).any(|l| at(&l))).copied().collect();
        let hit = |j: usize| matches!(self.records[j], Record::Block { func, n } if blocks.contains(&(func, n)));
        if dir > 0 { (i + 1..self.records.len()).find(|&j| hit(j)) } else { (0..i.min(self.records.len())).rev().find(|&j| hit(j)) }
    }

    /// The next block record after `i` (`other`: of another function than
    /// record `i`'s, if it is a block); `i` if none.
    pub fn next_block(&self, i: usize, other: bool) -> usize {
        let func_at = |j: usize| match self.records.get(j) {
            Some(Record::Block { func, .. }) => Some(*func),
            _ => None,
        };
        let here = func_at(i);
        (i + 1..self.len()).find(|&j| func_at(j).is_some_and(|f| !other || Some(f) != here)).unwrap_or(i)
    }

    /// The first call of function `name` in records `first..=last` (of the
    /// task at address `task`, or any): its record (see calls).
    pub fn first_call(&self, t: &Tables, name: &str, first: usize, last: usize, task: Option<u32>) -> Option<usize> {
        self.calls(t, |n| n == name, first, last, task).next()
    }

    /// The calls of the functions whose names `is` says yes to in records
    /// `first..=last` (of the task at address `task`, or any), in order:
    /// each one's record, its block 0; code inlined from one counts too,
    /// where it starts (a block that runs some, the task's block before not).
    pub fn calls<'a>(&'a self, t: &'a Tables, is: impl Fn(&str) -> bool, first: usize, last: usize, task: Option<u32>) -> impl Iterator<Item = usize> + 'a {
        use alloc::collections::BTreeSet;
        let ids: BTreeSet<u32> = t.functions.iter().filter(|(_, f)| is(&f.name)).map(|(&id, _)| id).collect();
        // The inline chains with one of them in them.
        let inl: BTreeSet<u32> = t.names.iter().enumerate().filter(|(_, n)| is(n)).map(|(k, _)| k as u32).collect();
        let chains: BTreeSet<u32> = t.inlines.iter().enumerate()
            .filter(|(_, c)| c.iter().any(|(k, _)| inl.contains(k))).map(|(c, _)| c as u32).collect();
        let inlined = move |func: u32, n: u32| !chains.is_empty() && t.block(func, n)
            .is_some_and(|b| b.steps.iter().any(|s| s.inlined.is_some_and(|c| chains.contains(&c))));
        // (Per task: whether its last block ran some inlined.)
        let mut was: BTreeMap<u32, bool> = BTreeMap::new();
        (first..self.len().min(last.saturating_add(1))).filter(move |&i| {
            let Record::Block { func, n } = self.records[i] else { return false };
            if task.is_some_and(|t| self.tasks[i] != t) {
                return false;
            }
            let here = inlined(func, n);
            let before = was.insert(self.tasks[i], here).unwrap_or(false);
            (n == 0 && ids.contains(&func)) || (here && !before)
        })
    }

    /// How often each block ran in syscall `n` (its task's records), or the
    /// whole recording (None).
    pub fn block_counts(&self, n: Option<usize>) -> BTreeMap<(u32, u32), u64> {
        let (first, last, task) = match n.map(|n| self.syscalls.get(n)) {
            Some(None) => return BTreeMap::new(),
            Some(Some(s)) => (s.first, s.last.unwrap_or(usize::MAX), Some(s.task)),
            None => (0, usize::MAX, None),
        };
        let mut counts = BTreeMap::new();
        for (i, r) in self.records.iter().enumerate().take(last.saturating_add(1)).skip(first) {
            if let Record::Block { func, n } = *r && task.is_none_or(|t| self.tasks[i] == t) {
                *counts.entry((func, n)).or_default() += 1;
            }
        }
        counts
    }

    /// The source lines run in records `first..=last` of the task at `task`.
    pub fn lines(&self, t: &Tables, first: usize, last: usize, task: u32) -> Vec<LineStep> {
        self.calls.lines(t, &self.records, &self.tasks, first, last, task)
    }

    /// The newest store to `addr..addr + len` at or before record `at` (or
    /// the end): (its record, the value it overwrote if kept, the line that
    /// made it: where the function running then was).
    pub fn last_write(&self, t: &Tables, addr: u32, len: u32, at: usize) -> Option<(usize, Option<u64>, Option<LineStep>)> {
        let at = at.min(self.len().checked_sub(1)?);
        let (i, old) = self.stores().last_write(&self.records, addr, len, at)?;
        Some((i, old, self.line_at(t, i)))
    }

    /// Where the store at record `i` copied its byte at `addr` from, if it
    /// is a copy (its record says where its bytes come from).
    pub fn copied_from(&self, i: usize, addr: u32) -> Option<u32> {
        match self.records.get(i)? {
            // (From where the store and the value overlap.)
            Record::Store { addr: at, from: Some(from), .. } => Some(from.get().wrapping_add(addr.max(*at) - at)),
            _ => None,
        }
    }

    /// The line the store at record `i` is on.
    pub fn line_at(&self, t: &Tables, i: usize) -> Option<LineStep> {
        self.calls.line_at(t, i, &self.records)
    }

    /// The stores in records `from..=to`, oldest first.
    pub fn stores_between(&self, from: usize, to: usize) -> &[usize] {
        self.stores().between(from, to.min(self.len().saturating_sub(1)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(func: u32, n: u32) -> Record {
        Record::Block { func, n }
    }

    #[test]
    fn tasks_switch_where_switch_to_returns() {
        let t = Tables::parse("F\t1\t__switch_to\t0:1\t3\n", "");
        let switch = t.find("__switch_to").unwrap();
        let r = [b(9, 1), b(switch, 0), Record::Return { func: switch, value: 0 }, b(9, 1)];
        let mut args = Args::default();
        args.push(1, &[0x100, 0x200, 0]);
        let rec = Recording::new(&t, r.to_vec(), (0..4).collect(), args);
        assert_eq!(rec.tasks, [0x100, 0x100, 0x200, 0x200]);
    }

    #[test]
    fn a_function_inlined_is_found_as_a_call() {
        // f (1): block 1 runs h's line 20 (h inlined into f).
        let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\nL\t1\t0\t0:1\n\
                               I\t0\th\t0:6\nL\t1\t1\t0:5;:20@0\n", "");
        let rec = Recording::new(&t, alloc::vec![b(1, 0), b(1, 1)], alloc::vec![0, 1], Args::default());
        assert_eq!((rec.first_call(&t, "f", 0, 1, None), rec.first_call(&t, "h", 0, 1, None)), (Some(0), Some(1)));
        // (A loop in the inlined code, blocks 1 and 1 again: one call, where it starts; f called again, and h in it.)
        let rec = Recording::new(&t, alloc::vec![b(1, 0), b(1, 1), b(1, 1), b(1, 0), b(1, 1)], (0..5).collect(), Args::default());
        assert_eq!((rec.calls(&t, |n| n == "f", 0, 4, None).collect::<Vec<_>>(), rec.calls(&t, |n| n == "h", 0, 4, None).collect::<Vec<_>>()), (alloc::vec![0, 3], alloc::vec![1, 4]));
    }

}
