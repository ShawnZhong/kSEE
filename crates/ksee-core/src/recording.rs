//! A recording: the records of the trace ring, and what the host rebuilds
//! from them alone: the task running at each record, the runs (each
//! task's syscalls, and what it did between them), and the
//! stores (memory as it was).

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::cell::OnceCell;

use crate::stack::{self, LineStep};
use crate::tables::Tables;
use crate::trace::{Args, Record};
use crate::{arch, mem, Memory};

/// A run of a task: a syscall it made, a call of the syscall entry
/// (arch::SYSCALL: its call record's arguments are the number and the
/// syscall's arguments; its return's value is the result); or what it did
/// between them (no number: the idle task's idling, a worker's work), from
/// its first record there to its last. What the cursor can be in.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub task: u32,
    pub nr: Option<u32>,
    /// The result (None: still running at the end of the trace, or it
    /// never returned).
    pub ret: Option<i32>,
    /// Its first and last records (a syscall's: its return's; none, still
    /// running as the records end).
    pub first: usize,
    pub last: Option<usize>,
}

/// The syscalls among `calls` (`entry`: the syscall entry's function id;
/// `tasks[i]`: the task running at record `i`), in the order they began.
fn split(entry: u32, records: &[Record], tasks: &[u32], args: &Args, calls: &stack::Calls) -> Vec<Run> {
    calls.calls.iter().filter(|c| c.func == entry && matches!(records[c.first], Record::Block { n: 0, .. })).map(|c| {
        let nr = args.get(c.first).and_then(|(w, _)| w.first().copied()).unwrap_or(0);
        let ret = c.last.and_then(|l| match records[l] {
            Record::Return { func, value: Some(value) } if func == entry => Some(value as i32),
            _ => None,
        });
        Run { task: tasks[c.first], nr: Some(nr), ret, first: c.first, last: ret.and(c.last) }
    }).collect()
}

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
    /// The runs, in the order they began.
    pub runs: Vec<Run>,
    /// The steps, in order: the recording's unit of time (the timeline's
    /// axis), a source line run, as stepping visits them (View::lines): its
    /// record, which of the lines the record's stretch runs it is, and the
    /// inlined frames it has not entered yet (LineStep::cut).
    pub steps: Vec<(usize, u32, u32)>,
    /// The calls, and where each record's task is in them.
    pub calls: stack::Calls,
    /// Who each task was (by its task_struct's address, and the last write
    /// to its pid or comm by then), read once per snapshot (View::task).
    pub(crate) task_ids: core::cell::RefCell<alloc::collections::BTreeMap<(u32, Option<usize>), crate::print::TaskId>>,
    /// The last span's stores, named (View::stores: what Objects and the
    /// source's notes ask again at each moment), by its first and last.
    pub(crate) span_stores: core::cell::RefCell<Option<((usize, usize), alloc::rc::Rc<crate::view::Named>)>>,
    /// The stores, indexed (once asked for).
    stores: OnceCell<mem::History>,
    /// Each call's frame depth (see frame_depth), once asked for.
    frames: OnceCell<Vec<u32>>,
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
        let mut runs = split(t.find(arch::SYSCALL).unwrap_or(0), &records, &tasks, &args, &calls);
        // (Each task's runs between its syscalls: its records there, first
        // to last.)
        let mut mine: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
        for (i, &task) in tasks.iter().enumerate() {
            mine.entry(task).or_default().push(i);
        }
        let mut gaps = Vec::new();
        for (&task, at) in &mine {
            // (One that ran code: a block; stores alone, a runner's wait, not.)
            let mut gap = |from: usize, to: usize| {
                let (a, b) = (at.partition_point(|&i| i < from), at.partition_point(|&i| i < to));
                if a < b && at[a..b].iter().any(|&i| matches!(records[i], Record::Block { .. })) {
                    gaps.push(Run { task, nr: None, ret: None, first: at[a], last: Some(at[b - 1]) });
                }
            };
            let mut from = 0;
            for s in runs.iter().filter(|s| s.task == task) {
                gap(from, s.first);
                from = s.last.map_or(records.len(), |l| l + 1);
            }
            gap(from, records.len());
        }
        runs.extend(gaps);
        runs.sort_by_key(|s| s.first);
        // (The lines each run ran, as stepping walks them: a call's width in
        // the chart is the steps it takes.)
        let mut steps: Vec<(usize, u32, u32)> = Vec::new();
        for s in &runs {
            let end = s.last.map_or(records.len(), |l| l + 1);
            steps.extend(calls.lines(t, &records, &tasks, s.first, end - 1, s.task).iter().map(|l| (l.record, l.nth, l.cut)));
        }
        steps.sort_by_key(|s| s.0);
        Recording { records, ids, args, tasks, runs, steps, calls, stores: OnceCell::new(), frames: OnceCell::new(), task_ids: Default::default(), span_stores: Default::default() }
    }

    /// Record `i`'s step: its last line's, so a record between two lines
    /// is at the earlier one's.
    pub fn step_of(&self, i: usize) -> usize {
        self.steps.partition_point(|&(r, _, _)| r <= i).saturating_sub(1)
    }

    /// The first step at or after record `i` (its first line's, if it runs any).
    pub fn first_step(&self, i: usize) -> usize {
        self.steps.partition_point(|&(r, _, _)| r < i)
    }

    /// The record step `k` is at, the last record if past the end.
    pub fn record_of(&self, k: usize) -> usize {
        self.steps.get(k).map_or(self.last(), |&(r, _, _)| r)
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// The last record's index (0: none).
    pub fn last(&self) -> usize {
        self.records.len().saturating_sub(1)
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

    /// The run of the task running at record `i` (its index): its syscall
    /// then, or what it did between.
    pub fn run_at(&self, i: usize) -> Option<usize> {
        let task = *self.tasks.get(i)?;
        self.runs.iter().position(|s| s.task == task && s.first <= i && s.last.is_none_or(|l| i <= l))
    }

    /// The first call of function `name` in records `first..=last` (of the
    /// task at address `task`, or any): its record (see calls).
    pub fn first_call(&self, t: &Tables, name: &str, first: usize, last: usize, task: Option<u32>) -> Option<usize> {
        self.find_calls(t, |n| n == name, first, last, task).next()
    }

    /// The calls of the functions whose names `is` says yes to in records
    /// `first..=last` (of the task at address `task`, or any), in order:
    /// each one's record, its block 0; code inlined from one counts too,
    /// where it starts (a block that runs some, the task's block before not).
    pub fn find_calls<'a>(&'a self, t: &'a Tables, is: impl Fn(&str) -> bool, first: usize, last: usize, task: Option<u32>) -> impl Iterator<Item = usize> + 'a {
        use alloc::collections::BTreeSet;
        let ids: BTreeSet<u32> = t.functions().filter(|(_, f)| is(f.name)).map(|(id, _)| id).collect();
        // The inline chains with one of them in them.
        let inl: BTreeSet<u32> = t.inline_names().enumerate().filter(|(_, n)| is(n)).map(|(k, _)| k as u32).collect();
        let chains: BTreeSet<u32> = (0..t.chains()).filter(|&c| t.chain(Some(c)).any(|s| inl.contains(&s.name))).collect();
        let inlined = move |func: u32, n: u32| !chains.is_empty() && t.block(func, n)
            .is_some_and(|b| b.steps.iter().any(|s| s.inlined().is_some_and(|c| chains.contains(&c))));
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

    /// How often each block ran in run `n` (its task's records), or the
    /// whole recording (None).
    pub(crate) fn block_counts(&self, n: Option<usize>) -> BTreeMap<(u32, u32), u64> {
        let (first, last, task) = match n.map(|n| self.runs.get(n)) {
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
    pub(crate) fn lines(&self, t: &Tables, first: usize, last: usize, task: u32) -> Vec<LineStep> {
        self.calls.lines(t, &self.records, &self.tasks, first, last, task)
    }

    /// The line the store at record `i` is on.
    pub(crate) fn line_at(&self, t: &Tables, i: usize) -> Option<LineStep> {
        self.calls.line_at(t, i, &self.records)
    }

    /// How many frames are outside call `k`'s own (an index in
    /// Calls::calls), calls and inlined code alike: its callers, and the code
    /// inlined into each at the line calling the next; its frame's place
    /// in a stack, from the outermost (Frame::depth). (A caller comes before
    /// its callees: one pass.)
    pub(crate) fn frame_depth(&self, t: &Tables, k: u32) -> u32 {
        let all = self.frames.get_or_init(|| {
            let mut out: Vec<u32> = Vec::with_capacity(self.calls.calls.len());
            for k in 0..self.calls.calls.len() as u32 {
                let d = self.calls.caller(t, k).map_or(0, |(parent, at)| {
                    out[parent as usize] + 1 + t.depth(at.step.and_then(|s| s.inlined()))
                });
                out.push(d);
            }
            out
        });
        all.get(k as usize).copied().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(func: u32, n: u32) -> Record {
        Record::Block { func, n }
    }

    #[test]
    fn syscalls_split_per_task() {
        // Task 7 makes syscall 64 (write): its entry (1) calls 2; task 8 runs meanwhile.
        let r = [Record::Block { func: 1, n: 0 }, Record::Block { func: 2, n: 0 }, Record::Block { func: 3, n: 0 },
                 Record::Return { func: 1, value: Some(3) }];
        let tasks = [7, 7, 8, 7];
        let mut args = Args::default();
        args.push(0, &[64, 1, 2, 3, 0]);
        let calls = stack::Calls::new(&crate::tables::Tables::default(), &r, &tasks);
        let s = split(1, &r, &tasks, &args, &calls);
        assert_eq!(s.len(), 1);
        assert_eq!((s[0].task, s[0].nr, s[0].ret, s[0].first, s[0].last), (7, Some(64), Some(3), 0, Some(3)));
    }

    #[test]
    fn tasks_switch_where_switch_to_returns() {
        let t = Tables::parse("F\t1\t__switch_to\t0:1\t3\n", "");
        let switch = t.find("__switch_to").unwrap();
        let r = [b(9, 1), b(switch, 0), Record::Return { func: switch, value: Some(0) }, b(9, 1)];
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
        assert_eq!((rec.find_calls(&t, |n| n == "f", 0, 4, None).collect::<Vec<_>>(), rec.find_calls(&t, |n| n == "h", 0, 4, None).collect::<Vec<_>>()), (alloc::vec![0, 3], alloc::vec![1, 4]));
    }

}
