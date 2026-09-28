//! Calls, rebuilt from the block trace: a shadow stack per task.
//!
//! Block 0 is a function's entry (LLVM's entry block has no predecessors,
//! so it runs once per call): it pushes a frame. A block of a function
//! further down the stack is a return to it: the frames above it pop. A
//! block of a function not on the stack (its entry was before the trace
//! began) starts the stack again from it. A return record pops the
//! function's frame (and any above it). A task starts (wasm_task_entry)
//! with nothing on its stack: a task that exited never returns from its
//! last calls (do_exit, do_task_dead, __switch_to), and a new task may get
//! its task_struct.

use alloc::borrow::Cow;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::tables::{Step, Tables};
use crate::trace::Record;

/// No call, or no place.
const NONE: u32 = u32::MAX;

/// A call: `func` from record `first` (its block 0; or where the recording
/// first sees it running) to `last`, the record that ends it (its return,
/// or where its task's stack starts again), if it ends within the
/// recording; made from `parent`, `depth` calls deep.
#[derive(Debug, Clone, Copy)]
pub struct Call {
    pub func: u32,
    pub first: usize,
    pub last: Option<usize>,
    pub parent: Option<u32>,
    pub depth: u32,
    /// Where its caller was when it made it (a Place), or NONE.
    site: u32,
}

/// Where a call is: in block `n`, running stretch `seg` of the block's
/// lines (Tables::segments) from `record` (the block's, or the return into
/// it); at the last line of stretch `line` (seg, or an earlier one if seg
/// has none; NONE: no line yet).
#[derive(Debug, Clone, Copy)]
struct Place {
    call: u32,
    n: u32,
    seg: u32,
    line: u32,
    record: u32,
}

/// The calls of a recording, rebuilt once from its blocks and returns: every
/// call, and where each record's task is (its innermost call, and where in
/// its lines). The stack at a record, where a call ends, the line a store
/// is on: lookups, not walks.
#[derive(Debug, Default)]
pub struct Calls {
    pub calls: Vec<Call>,
    /// Per record: where its task is after it (a Place), NONE if nowhere.
    at: Vec<u32>,
    places: Vec<Place>,
}

impl Calls {
    pub fn new(t: &Tables, records: &[Record], tasks: &[u32]) -> Calls {
        let mut c = Calls { calls: Vec::new(), at: Vec::with_capacity(records.len()), places: Vec::new() };
        // (Per task: its calls on the stack, outermost first, and where the innermost is.)
        let mut stacks: BTreeMap<u32, (Vec<u32>, u32)> = BTreeMap::new();
        let entry = Tables::id(crate::arch::TASK_ENTRY);
        for (i, r) in records.iter().enumerate() {
            let (frames, place) = stacks.entry(tasks[i]).or_insert((Vec::new(), NONE));
            match *r {
                Record::Block { func, n } => {
                    let top = frames.last().map(|&k| c.calls[k as usize].func);
                    if n == 0 || top != Some(func) {
                        // A call pushes a frame (a new task's first: on nothing);
                        // a block further down the stack is a return into it; a
                        // block of a function not on it starts it again.
                        let down = frames.iter().rposition(|&k| c.calls[k as usize].func == func);
                        let keep = match (n, down) {
                            (0, _) if func == entry => 0,
                            (0, _) => frames.len(),
                            (_, Some(k)) => k + 1,
                            (_, None) => 0,
                        };
                        c.end(frames, keep, i);
                        if n == 0 || down.is_none() {
                            let parent = frames.last().copied();
                            let site = if parent.is_some() { *place } else { NONE };
                            c.calls.push(Call { func, first: i, last: None, parent, depth: frames.len() as u32, site });
                            frames.push(c.calls.len() as u32 - 1);
                        }
                    }
                    let call = *frames.last().expect("a frame");
                    let line = if t.stretch(func, n, 0).is_empty() { NONE } else { 0 };
                    *place = c.place(Place { call, n, seg: 0, line, record: i as u32 });
                }
                Record::Return { func, .. } => {
                    if let Some(k) = frames.iter().rposition(|&k| c.calls[k as usize].func == func) {
                        // (The caller goes on after the call: its block's next stretch.)
                        let site = c.calls[frames[k] as usize].site;
                        c.end(frames, k, i);
                        *place = match frames.last() {
                            Some(&call) if site != NONE && c.places[site as usize].call == call => {
                                let p = c.places[site as usize];
                                let seg = p.seg + 1;
                                let func = c.calls[call as usize].func;
                                let line = if t.stretch(func, p.n, seg).is_empty() { p.line } else { seg };
                                c.place(Place { call, n: p.n, seg, line, record: i as u32 })
                            }
                            Some(&call) => c.place(Place { call, n: NONE, seg: 0, line: NONE, record: i as u32 }),
                            None => NONE,
                        };
                    }
                }
                _ => {}
            }
            c.at.push(*place);
        }
        c
    }

    fn place(&mut self, p: Place) -> u32 {
        self.places.push(p);
        self.places.len() as u32 - 1
    }

    /// End the calls on `frames` above the first `keep`, at record `i`.
    fn end(&mut self, frames: &mut Vec<u32>, keep: usize, i: usize) {
        for &k in frames.iter().skip(keep) {
            self.calls[k as usize].last = Some(i);
        }
        frames.truncate(keep);
    }

    /// Where record `i`'s task is after it.
    fn place_at(&self, i: usize) -> Option<&Place> {
        self.places.get(*self.at.get(i)? as usize)
    }

    /// The line place `p`'s call is at (the last of its stretch).
    fn line(&self, t: &Tables, p: &Place) -> Option<Step> {
        if p.line == NONE {
            return None;
        }
        t.stretch(self.calls[p.call as usize].func, p.n, p.line).last().copied()
    }

    /// The lines record `i` runs, if it runs some (a block, or a return
    /// into the middle of one): (its place, the lines).
    fn stretch<'t>(&self, t: &'t Tables, i: usize) -> Option<(&Place, Cow<'t, [Step]>)> {
        let p = self.place_at(i).filter(|p| p.record as usize == i && p.n != NONE)?;
        Some((p, t.stretch(self.calls[p.call as usize].func, p.n, p.seg)))
    }

    /// The call the block 0 at record `first` begins.
    pub fn call_from(&self, first: usize) -> Option<&Call> {
        let c = self.calls.get(self.place_at(first)?.call as usize)?;
        (c.first == first).then_some(c)
    }

    /// Where the call from record `first` (a block 0) ends: the record
    /// that takes it off its task's stack (its return), or None if it has
    /// not ended when the records do.
    pub fn call_end(&self, first: usize) -> Option<usize> {
        self.call_from(first)?.last
    }

    /// The call stack of the task running at record `i`, outermost first,
    /// each call at its last line; the innermost, if `nth`, at the nth of the
    /// lines record `i` runs (a LineStep's).
    pub fn stack_at(&self, t: &Tables, i: usize, nth: Option<usize>) -> Vec<ChartCall> {
        let Some(p) = self.place_at(i) else { return Vec::new() };
        let mut step = match (nth, self.stretch(t, i)) {
            (Some(k), Some((_, steps))) if k < steps.len() => Some(steps[k]),
            _ => self.line(t, p),
        };
        let mut out = Vec::new();
        let mut k = Some(p.call);
        while let Some(c) = k.map(|k| &self.calls[k as usize]) {
            out.push(ChartCall { func: c.func, first: c.first, step });
            step = self.places.get(c.site as usize).and_then(|p| self.line(t, p));
            k = c.parent;
        }
        out.reverse();
        out
    }

    /// The line the store at record `i` is on (its record's line: which of
    /// its block's lines), in the call running it.
    pub fn line_at(&self, t: &Tables, i: usize, records: &[Record]) -> Option<LineStep> {
        let &Record::Store { line, .. } = records.get(i)? else { return None };
        let p = self.place_at(i)?;
        let c = &self.calls[p.call as usize];
        // (Which stretch it is in, and where in it.)
        let (mut seg, mut nth) = (0, line as usize);
        let s = loop {
            let steps = t.stretch(c.func, p.n, seg);
            match steps.get(nth) {
                Some(&s) => break s,
                None if steps.is_empty() && seg > p.seg => return None,
                None => (seg, nth) = (seg + 1, nth - steps.len()),
            }
        };
        let nth = nth as u32;
        Some(LineStep { record: p.record as usize, at: i, nth, func: c.func, depth: c.depth, loc: s.loc, inlined: s.inlined })
    }

    /// A stack chart: for each column (its first record, in `columns`, in
    /// order: see Recording::columns), the task running there and that
    /// task's call stack then, outermost first.
    pub fn chart(&self, t: &Tables, tasks: &[u32], columns: &[usize]) -> Vec<(u32, Vec<ChartCall>)> {
        columns.iter().filter(|&&i| i < tasks.len()).map(|&i| (tasks[i], self.stack_at(t, i, None))).collect()
    }

    /// The source lines run in records `from..=to` (of task `task`, or
    /// any), in order: each block's lines up to its first call, then, as
    /// each call returns, the lines up to the next (Tables::segments). The
    /// same line twice in a row counts once.
    pub fn lines(&self, t: &Tables, records: &[Record], tasks: &[u32], from: usize, to: usize,
                 task: Option<u32>) -> Vec<LineStep> {
        let mut out: Vec<LineStep> = Vec::new();
        for i in from..=to.min(records.len().saturating_sub(1)) {
            let tk = tasks[i];
            if task.is_some_and(|x| x != tk) {
                continue;
            }
            let Some((p, steps)) = self.stretch(t, i) else { continue };
            let c = &self.calls[p.call as usize];
            // (The stretch's lines are numbered on from the block's earlier
            // ones: a store says which it is on; the first store of each line,
            // among the records up to the task's next block or return.)
            let base: usize = (0..p.seg).map(|k| t.stretch(c.func, p.n, k).len()).sum();
            let mut first_store: BTreeMap<u32, usize> = BTreeMap::new();
            for (j, r) in records.iter().enumerate().skip(i + 1) {
                if tasks[j] != tk {
                    continue;
                }
                match *r {
                    Record::Store { line, .. } => { first_store.entry(line).or_insert(j); }
                    _ => break,
                }
            }
            for (nth, s) in steps.iter().enumerate() {
                if out.last().is_some_and(|l| l.loc.file == s.loc.file && l.loc.line == s.loc.line && l.inlined == s.inlined) {
                    continue;
                }
                let at = if nth == 0 { i } else { first_store.get(&((base + nth) as u32)).copied().unwrap_or(i) };
                out.push(LineStep { record: i, at, nth: nth as u32, func: c.func, depth: c.depth, loc: s.loc, inlined: s.inlined });
            }
        }
        out
    }

    /// The calls of task `task` in records `from..=to` as a tree, in order:
    /// what a syscall did, with the code inlined into each call as calls of
    /// its own (see CallNode::inlined). Calls on the stack before `from` are
    /// not in it (a syscall's entry, say: its callees are the roots).
    pub fn call_tree(&self, t: &Tables, records: &[Record], tasks: &[u32], from: usize, to: usize, task: u32) -> Vec<CallNode> {
        let to = to.min(records.len().saturating_sub(1));
        // (The nodes open, outermost first, each by what it is: a call's first
        // record, or an inlined function; and the finished ones at the top.)
        let mut open: Vec<((bool, usize), CallNode)> = Vec::new();
        let mut roots: Vec<CallNode> = Vec::new();
        let close = |open: &mut Vec<((bool, usize), CallNode)>, roots: &mut Vec<CallNode>, i: usize| {
            let (_, mut c) = open.pop().expect("an open call");
            c.last = i;
            c.records = (i - c.first + 1) as u32;
            if let Some(&Record::Return { func, value }) = records.get(i) && !c.inlined && func == c.func {
                c.ret = Some(value as i32);
            }
            match open.last_mut() {
                Some((_, p)) => p.calls.push(c),
                None => roots.push(c),
            }
        };
        for i in from..=to {
            if tasks[i] != task || self.stretch(t, i).is_none() {
                continue;
            }
            // What is on the stack since `from`: each call, then the code
            // inlined at the line it is at (outermost first), as the chart nests
            // them; against the nodes open: the ones gone close, new ones open.
            let mut want: Vec<(bool, usize, u32)> = Vec::new();
            for c in self.stack_at(t, i, None).iter().filter(|c| c.first >= from) {
                want.push((false, c.first, c.func));
                let chain = c.step.and_then(|s| s.inlined).and_then(|k| t.inlines.get(k as usize)).map_or(&[][..], Vec::as_slice);
                want.extend(chain.iter().rev().map(|&(k, _)| (true, k as usize, k)));
            }
            let mut k = 0;
            while k < open.len() && k < want.len() && open[k].0 == (want[k].0, want[k].1) {
                k += 1;
            }
            while open.len() > k {
                close(&mut open, &mut roots, i);
            }
            for &(inlined, id, func) in &want[k..] {
                open.push(((inlined, id), CallNode { func, inlined, first: i, last: i, records: 1, ret: None, calls: Vec::new() }));
            }
        }
        while !open.is_empty() {
            close(&mut open, &mut roots, to);
        }
        roots
    }
}

/// A call in a stack chart: `func` from record `first`, at `step` (its last
/// line then, if known: with the functions it is inlined from).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChartCall {
    pub func: u32,
    pub first: usize,
    pub step: Option<Step>,
}

/// A source line the kernel ran: at `record` (the block's, or the return
/// after which the block goes on), the `nth` of the lines it runs there, in
/// `func` (`inlined` from other functions, maybe: see Tables::inlines),
/// `depth` calls deep. The line is `at` the record of its first store
/// (Record::Store's line), or, with no store or the first of its stretch,
/// the block's: where the cursor goes, and memory as it was there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineStep {
    pub record: usize,
    pub at: usize,
    pub nth: u32,
    pub func: u32,
    pub depth: u32,
    pub loc: crate::tables::Loc,
    pub inlined: Option<u32>,
}


/// The calls of any of `funcs` in records `from..=to`: each call's first
/// record (its block 0), in order.
pub fn calls_of(records: &[Record], from: usize, to: usize, funcs: &[u32]) -> Vec<usize> {
    let to = to.min(records.len().saturating_sub(1));
    (from..=to).filter(|&i| matches!(records[i], Record::Block { func, n: 0 } if funcs.contains(&func))).collect()
}

/// A call in a call tree: `func` from record `first` to `last` (its
/// return, or where the records end), with the calls it made (`calls`, in
/// order); `records`: how many records are its, its callees' included.
/// `inlined`: not a call but code inlined into the call before it, from
/// function `func` (an index into Tables::names), as the stack chart and
/// the call stack show it.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct CallNode {
    pub func: u32,
    pub inlined: bool,
    pub first: usize,
    pub last: usize,
    pub records: u32,
    /// What the call returned (its Return record's value), if it did
    /// within the span (not for inlined code, which does not return).
    pub ret: Option<i32>,
    pub calls: Vec<CallNode>,
}


#[cfg(test)]
mod tests {
    use super::*;

    fn b(func: u32, n: u32) -> Record {
        Record::Block { func, n }
    }

    fn stack_at(records: &[Record], tasks: &[u32], at: usize) -> Vec<u32> {
        let t = Tables::default();
        Calls::new(&t, records, tasks).stack_at(&t, at, None).into_iter().map(|c| c.func).collect()
    }

    fn lines(t: &Tables, records: &[Record], tasks: &[u32], from: usize, to: usize, task: Option<u32>) -> Vec<LineStep> {
        Calls::new(t, records, tasks).lines(t, records, tasks, from, to, task)
    }

    fn call_tree(t: &Tables, records: &[Record], tasks: &[u32], from: usize, to: usize, task: u32) -> Vec<CallNode> {
        Calls::new(t, records, tasks).call_tree(t, records, tasks, from, to, task)
    }

    fn call_end(records: &[Record], tasks: &[u32], first: usize) -> Option<usize> {
        Calls::new(&Tables::default(), records, tasks).call_end(first)
    }

    #[test]
    fn calls_nest_and_return() {
        // f calls g, g calls h; back in g, back in f; then f calls g again.
        let r = [b(1, 0), b(1, 1), b(2, 0), b(3, 0), b(2, 1), b(1, 2), b(2, 0), b(1, 3)];
        let t = [7; 8];
        assert_eq!(stack_at(&r, &t, 3), [1, 2, 3]);
        assert_eq!(stack_at(&r, &t, 5), [1]);
    }

    #[test]
    fn lines_go_on_after_calls() {
        // f's block 0: lines 1, 2, a call (g: line 10), then line 3.
        let t = crate::tables::Tables::parse(
            "U\nP\t0\tf.c\nP\t1\tg.c\nB\t1\t0\t0:1:1\nL\t1\t0\t0:1;:2;|;:3\nB\t2\t0\t1:10:1\n", "");
        let r = [b(1, 0), b(2, 0), Record::Return { func: 2, value: 0 }, Record::Return { func: 1, value: 0 }];
        let got: Vec<_> = lines(&t, &r, &[7; 4], 0, 3, None).iter().map(|s| (s.loc.line, s.record, s.depth)).collect();
        assert_eq!(got, [(1, 0, 0), (2, 0, 0), (10, 1, 1), (3, 2, 0)]);
    }

    #[test]
    fn lines_are_at_their_first_store() {
        // f's block 0 runs lines 1, 2, 3, then calls g (line 10), then line 4;
        // line 2 stores twice, line 4 once: a line is at its first store,
        // but a stretch's first line at the block, or the return, itself.
        let t = crate::tables::Tables::parse(
            "U\nP\t0\tf.c\nP\t1\tg.c\nB\t1\t0\t0:1:1\nL\t1\t0\t0:1;:2;:3;|;:4\nB\t2\t0\t1:10:1\n", "");
        let st = |line| Record::Store { addr: 8, size: 4, old: 0, line };
        let r = [b(1, 0), st(1), st(1), b(2, 0), Record::Return { func: 2, value: 0 }, st(3), Record::Return { func: 1, value: 0 }];
        let got: Vec<_> = lines(&t, &r, &[7; 7], 0, 6, None).iter().map(|s| (s.loc.line, s.record, s.at)).collect();
        assert_eq!(got, [(1, 0, 0), (2, 0, 1), (3, 0, 0), (10, 3, 3), (4, 4, 4)]);
        // A store is on its line (the block's third item, line 4, after the call); the
        // stack there has f at line 4, and at g, f where it calls g (line 3).
        let c = Calls::new(&t, &r, &[7; 7]);
        let at = |i| c.line_at(&t, i, &r).map(|s| (s.loc.line, s.nth, s.record));
        assert_eq!((at(1), at(5), at(0)), (Some((2, 1, 0)), Some((4, 0, 4)), None));
        let lines_of = |i| c.stack_at(&t, i, None).iter().map(|c| c.step.map(|s| s.loc.line)).collect::<Vec<_>>();
        assert_eq!((lines_of(3), lines_of(5)), (alloc::vec![Some(3), Some(10)], alloc::vec![Some(4)]));
    }

    #[test]
    fn a_new_task_starts_empty() {
        // A task that exited in f (never returning), then a new task at its address.
        let entry = Tables::id(crate::arch::TASK_ENTRY);
        let r = [b(entry, 0), b(1, 0), b(entry, 0), b(2, 0)];
        assert_eq!(stack_at(&r, &[7; 4], 3), [entry, 2]);
    }

    #[test]
    fn a_call_tree_nests_calls_and_inlined_code() {
        // 1 calls 2 (which calls 3, then returns) and then 4; task 8 runs between.
        let t = Tables::default();
        let ret = |f| Record::Return { func: f, value: 0 };
        let r = [b(1, 0), b(2, 0), b(3, 0), b(9, 0), ret(3), ret(2), b(1, 1), b(4, 0), ret(4), ret(1)];
        let tk = [7, 7, 7, 8, 7, 7, 7, 7, 7, 7];
        let tree = call_tree(&t, &r, &tk, 0, 9, 7);
        let shape: Vec<_> = tree.iter().map(|c| (c.func, c.calls.iter().map(|d| (d.func, d.calls.len(), d.records)).collect::<Vec<_>>())).collect();
        assert_eq!(shape, [(1, alloc::vec![(2, 1, 5), (4, 0, 2)])]);
        // (What each returned: 2 and 4 did, 0; 1 too, at the span's end.)
        assert_eq!((tree[0].ret, tree[0].calls[0].ret, tree[0].calls[1].ret), (Some(0), Some(0), Some(0)));
        // From record 1 on: 1 is an ancestor, not in the tree.
        let tree = call_tree(&t, &r, &tk, 1, 9, 7);
        assert_eq!(tree.iter().map(|c| (c.func, c.calls.len())).collect::<Vec<_>>(), [(2, 1), (4, 0)]);
        // f (1) runs line 1, then h's line 20 (h inlined into f at line 2), which calls g (2):
        // h is a node of its own, inlined, and g is its call.
        let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\nF\t2\tg\t0:9\t0\nB\t1\t0\t0:1:1\n\
                               I\t0\th\t0:2\nL\t1\t0\t0:1;:20@0;|\nB\t2\t0\t0:10:1\n", "");
        let r = [b(1, 0), b(2, 0), ret(2), ret(1)];
        let tree = call_tree(&t, &r, &[7; 4], 0, 3, 7);
        let h = &tree[0].calls[0];
        assert_eq!((tree[0].func, tree[0].inlined, h.inlined, t.names[h.func as usize].as_str(), h.calls[0].func), (1, false, true, "h", 2));
    }

    #[test]
    fn calls_of_a_function_are_its_entries() {
        let r = [b(1, 0), b(2, 0), b(2, 1), b(1, 1), b(2, 0)];
        assert_eq!(calls_of(&r, 0, 4, &[2]), [1, 4]);
        assert_eq!(calls_of(&r, 2, 4, &[2]), [4]);
    }

    #[test]
    fn calls_end_where_they_return() {
        // f (1) calls g (2), which returns; f goes on, then returns. (Task 8 runs between.)
        let r = [b(1, 0), b(2, 0), b(9, 0), Record::Return { func: 2, value: 0 }, b(1, 1), Record::Return { func: 1, value: 0 }, b(3, 0)];
        let t = [7, 7, 8, 7, 7, 7, 7];
        assert_eq!((call_end(&r, &t, 1), call_end(&r, &t, 0), call_end(&r, &t, 6)), (Some(3), Some(5), None));
    }

    #[test]
    fn exits_pop() {
        // A syscall (1) that calls 2, then returns to user space; another.
        let exit = Record::Return { func: 1, value: 0 };
        let r = [b(1, 0), b(2, 0), exit, b(1, 0)];
        assert_eq!(stack_at(&r, &[7; 4], 3), [1]);
    }

    #[test]
    fn tasks_have_their_own_stacks() {
        // Task 7 is in f when task 8 runs g; back on 7, f goes on.
        let r = [b(1, 0), b(2, 0), b(2, 1), b(1, 1)];
        let t = [7, 8, 8, 7];
        assert_eq!(stack_at(&r, &t, 3), [1]);
    }

    #[test]
    fn a_return_past_the_start_restarts_the_stack() {
        let r = [b(2, 3), b(3, 0), b(1, 4)];
        assert_eq!(stack_at(&r, &[7; 3], 1), [2, 3]);
        assert_eq!(stack_at(&r, &[7; 3], 2), [1]);
    }
}
