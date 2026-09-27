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

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::tables::{Step, Tables};
use crate::trace::Record;

#[derive(Default)]
struct Stack {
    /// (function, first record)
    frames: Vec<(u32, usize)>,
}

impl Stack {
    /// Apply block `(func, n)` at record `i`.
    fn block(&mut self, func: u32, n: u32, i: usize) {
        if n == 0 {
            if func == Tables::id(crate::arch::TASK_ENTRY) {
                self.frames.clear(); // (a new task)
            }
            self.frames.push((func, i));
        } else if self.frames.last().is_some_and(|&(f, _)| f == func) {
            // Still in the same function.
        } else if let Some(k) = self.frames.iter().rposition(|&(f, _)| f == func) {
            self.frames.truncate(k + 1);
        } else {
            self.frames.clear();
            self.frames.push((func, i));
        }
    }

    /// Function `func` returned.
    fn exit(&mut self, func: u32) {
        if let Some(k) = self.frames.iter().rposition(|&(f, _)| f == func) {
            self.frames.truncate(k);
        }
    }
}

/// A task's calls, with where each is in its function: the walk that
/// `lines`, `chart` and `stack_at` share.
#[derive(Default)]
struct Walk {
    stack: Stack,
    /// Per frame: its block, and the stretch of the block's lines next.
    blocks: Vec<(u32, usize)>,
    /// Per frame: the last line it ran.
    at: Vec<Option<Step>>,
}

impl Walk {
    /// The calls now, outermost first, each at its last line.
    fn calls(&self) -> Vec<ChartCall> {
        self.stack.frames.iter().enumerate()
            .map(|(d, &(func, first))| ChartCall { func, first, step: self.at.get(d).copied().flatten() }).collect()
    }

    /// Apply record `r` (at `i`): the lines it runs, if any: (function,
    /// depth, lines), a stretch of a block.
    fn apply(&mut self, t: &Tables, r: &Record, i: usize) -> Option<(u32, u32, Vec<Step>)> {
        let (func, n, seg) = match *r {
            Record::Block { func, n } => {
                self.stack.block(func, n, i);
                let depth = self.stack.frames.len();
                self.blocks.resize(depth, (u32::MAX, 0));
                self.blocks[depth - 1] = (n, 1);
                (func, n, 0)
            }
            Record::Return { func, .. } => {
                self.stack.exit(func);
                self.blocks.truncate(self.stack.frames.len());
                // The caller goes on after the call: its block's next stretch.
                let (&(f, _), (n, seg)) = (self.stack.frames.last()?, self.blocks.last_mut()?);
                if *n == u32::MAX {
                    return None;
                }
                *seg += 1;
                (f, *n, *seg - 1)
            }
            _ => return None,
        };
        let depth = self.stack.frames.len();
        let steps = t.segments(func, n).into_iter().nth(seg).unwrap_or_default();
        self.at.resize(depth, None);
        // (A new call starts without a line; one going on keeps its last.)
        if steps.last().is_some() || seg == 0 {
            self.at[depth - 1] = steps.last().copied();
        }
        Some((func, depth as u32 - 1, steps))
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

/// A stack chart: for each column (its first record, in `columns`, in
/// order: see Recording::columns), the task running there and that task's
/// call stack then, outermost first.
pub fn chart(t: &Tables, records: &[Record], tasks: &[u32], columns: &[usize]) -> Vec<(u32, Vec<ChartCall>)> {
    let mut out = Vec::with_capacity(columns.len());
    let Some(&to) = columns.last() else { return out };
    let mut walks: BTreeMap<u32, Walk> = BTreeMap::new();
    let mut col = 0;
    for (i, r) in records.iter().enumerate().take(to.min(records.len().saturating_sub(1)) + 1) {
        let task = tasks[i];
        let w = walks.entry(task).or_default();
        w.apply(t, r, i);
        // (A column's first record, or several columns' when zoomed in.)
        while col < columns.len() && i >= columns[col] {
            out.push((task, w.calls()));
            col += 1;
        }
    }
    out
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

/// The source lines run in records `from..=to` (of task `task`, or any), in
/// order: each block's lines up to its first call, then, as each call
/// returns, the lines up to the next (Tables::segments). The same line twice
/// in a row counts once.
pub fn lines(t: &Tables, records: &[Record], tasks: &[u32], from: usize, to: usize,
             task: Option<u32>) -> Vec<LineStep> {
    let mut out: Vec<LineStep> = Vec::new();
    let to = to.min(records.len().saturating_sub(1));
    let mut walks: BTreeMap<u32, Walk> = BTreeMap::new();
    for (i, r) in records.iter().enumerate().take(to + 1) {
        let tk = tasks[i];
        if task.is_some_and(|x| x != tk) {
            continue;
        }
        let w = walks.entry(tk).or_default();
        let Some((func, depth, steps)) = w.apply(t, r, i) else { continue };
        if i < from {
            continue;
        }
        // (The stretch's lines are numbered on from the block's earlier
        // ones: a store says which it is on; the first store of each line,
        // among the records up to the task's next block or return.)
        let (n, seg) = w.blocks[depth as usize];
        let base: usize = t.segments(func, n).iter().take(seg.saturating_sub(1) as usize).map(Vec::len).sum();
        let mut first_store: BTreeMap<u32, usize> = BTreeMap::new();
        for (j, r) in records.iter().enumerate().skip(i + 1) {
            if tasks[j] != tk {
                continue;
            }
            match *r {
                Record::Store { line, .. } => { first_store.entry(line).or_insert(j); }
                Record::Args { .. } => {}
                _ => break,
            }
        }
        for (nth, s) in steps.into_iter().enumerate() {
            if out.last().is_some_and(|l| l.loc.file == s.loc.file && l.loc.line == s.loc.line && l.inlined == s.inlined) {
                continue;
            }
            let at = if nth == 0 { i } else { first_store.get(&((base + nth) as u32)).copied().unwrap_or(i) };
            out.push(LineStep { record: i, at, nth: nth as u32, func, depth, loc: s.loc, inlined: s.inlined });
        }
    }
    out
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
    pub calls: Vec<CallNode>,
}

/// The calls of task `task` in records `from..=to` as a tree, in order:
/// what a syscall did, with the code inlined into each call as calls of
/// its own (see CallNode::inlined). Calls on the stack before `from` are
/// not in it (a syscall's entry, say: its callees are the roots).
pub fn call_tree(t: &Tables, records: &[Record], tasks: &[u32], from: usize, to: usize, task: u32) -> Vec<CallNode> {
    let to = to.min(records.len().saturating_sub(1));
    let mut w = Walk::default();
    // (The nodes open, outermost first, each by what it is: a call's first
    // record, or an inlined function; and the finished ones at the top.)
    let mut open: Vec<((bool, usize), CallNode)> = Vec::new();
    let mut roots: Vec<CallNode> = Vec::new();
    let close = |open: &mut Vec<((bool, usize), CallNode)>, roots: &mut Vec<CallNode>, i: usize| {
        let (_, mut c) = open.pop().expect("an open call");
        c.last = i;
        c.records = (i - c.first + 1) as u32;
        match open.last_mut() {
            Some((_, p)) => p.calls.push(c),
            None => roots.push(c),
        }
    };
    for (i, r) in records.iter().enumerate().take(to + 1) {
        if tasks[i] != task {
            continue;
        }
        if w.apply(t, r, i).is_none() || i < from {
            continue;
        }
        // What is on the stack since `from`: each call, then the code
        // inlined at the line it is at (outermost first), as the chart nests
        // them; against the nodes open: the ones gone close, new ones open.
        let mut want: Vec<(bool, usize)> = Vec::new();
        for (d, &(_, first)) in w.stack.frames.iter().enumerate() {
            if first < from {
                continue;
            }
            want.push((false, first));
            let chain = w.at.get(d).copied().flatten().and_then(|s| s.inlined).map_or(&[][..], |k| t.inlines.get(k as usize).map_or(&[][..], Vec::as_slice));
            want.extend(chain.iter().rev().map(|&(k, _)| (true, k as usize)));
        }
        let mut k = 0;
        while k < open.len() && k < want.len() && open[k].0 == want[k] {
            k += 1;
        }
        while open.len() > k {
            close(&mut open, &mut roots, i);
        }
        for &(inlined, id) in &want[k..] {
            let func = if inlined { id as u32 } else { w.stack.frames.iter().find(|f| f.1 == id).map_or(0, |f| f.0) };
            open.push(((inlined, id), CallNode { func, inlined, first: i, last: i, records: 1, calls: Vec::new() }));
        }
    }
    while !open.is_empty() {
        close(&mut open, &mut roots, to);
    }
    roots
}

/// Where the call from record `first` (a block 0) ends: the last record of
/// its task's while it is on the stack (its return), or None if it has not
/// ended when the records do.
pub fn call_end(records: &[Record], tasks: &[u32], first: usize) -> Option<usize> {
    let &task = tasks.get(first)?;
    let mut s = Stack::default();
    let mut depth = None;
    for (i, r) in records.iter().enumerate() {
        if tasks[i] != task {
            continue;
        }
        match *r {
            Record::Block { func, n } => s.block(func, n, i),
            Record::Return { func, .. } => s.exit(func),
            _ => continue,
        }
        match depth {
            None if i == first => depth = Some(s.frames.len()),
            // (Gone: returned, or its stack started again.)
            Some(d) if s.frames.get(d - 1).is_none_or(|&(_, at)| at != first) => return Some(i),
            _ => {}
        }
    }
    None
}

/// The call stack of the task running at record `at`, outermost first,
/// each call at its last line; the innermost, if `nth`, at the nth of the
/// lines record `at` runs (a LineStep's).
pub fn stack_at(t: &Tables, records: &[Record], tasks: &[u32], at: usize, nth: Option<usize>) -> Vec<ChartCall> {
    let Some(&task) = tasks.get(at) else { return Vec::new() };
    let mut w = Walk::default();
    for (i, r) in records.iter().enumerate().take(at + 1) {
        if tasks[i] != task {
            continue;
        }
        let Some((_, depth, steps)) = w.apply(t, r, i) else { continue };
        if let (true, Some(&s)) = (i == at, nth.and_then(|k| steps.get(k))) {
            w.at[depth as usize] = Some(s);
        }
    }
    w.calls()
}

/// Call `first`'s arguments and its frame base (the Args record after its
/// entry block), if the trace has them.
pub fn args_frame(records: &[Record], first: usize) -> Option<(Vec<u32>, u32)> {
    match records.get(first + 1) {
        Some(&Record::Args { args, n, frame }) if matches!(records[first], Record::Block { n: 0, .. }) => Some((args[..n as usize].to_vec(), frame)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(func: u32, n: u32) -> Record {
        Record::Block { func, n }
    }

    fn stack_at(records: &[Record], tasks: &[u32], at: usize) -> Vec<u32> {
        super::stack_at(&Tables::default(), records, tasks, at, None).into_iter().map(|c| c.func).collect()
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
        // From record 1 on: 1 is an ancestor, not in the tree.
        let tree = call_tree(&t, &r, &tk, 1, 9, 7);
        assert_eq!(tree.iter().map(|c| (c.func, c.calls.len())).collect::<Vec<_>>(), [(2, 1), (4, 0)]);
        // f (1) runs line 1, then h's line 20 (h inlined into f at line 2), which calls g (2):
        // h is a node of its own, inlined, and g is its call.
        let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\nF\t2\tg\t0:9\t0\nB\t1\t0\t0:1:1\n\
                               L\t1\t0\t0:1;:20@h^0:2;|\nB\t2\t0\t0:10:1\n", "");
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
