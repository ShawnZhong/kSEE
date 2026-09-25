//! Calls, rebuilt from the block trace: a shadow stack per task.
//!
//! Block 0 is a function's entry (LLVM's entry block has no predecessors,
//! so it runs once per call): it pushes a frame. A block of a function
//! further down the stack is a return to it: the frames above it pop. A
//! block of a function not on the stack (its entry was before the trace
//! began) starts the stack again from it. A return record pops the
//! function's frame (and any above it); so does an exit record (a function
//! with an exit probe, like wasm_syscall).

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::trace::Record;

/// A call: `func` at `depth` (0: outermost seen), from record `first` to
/// record `last` (the last one of its task while it was on the stack).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame {
    pub func: u32,
    pub depth: u32,
    pub first: usize,
    pub last: usize,
}

#[derive(Default)]
struct Stack {
    /// (function, first record)
    frames: Vec<(u32, usize)>,
    /// The task's latest record.
    last: usize,
}

impl Stack {
    /// Apply block `(func, n)` at record `i`; `pop` gets each frame that ends.
    fn block(&mut self, func: u32, n: u32, i: usize, pop: &mut impl FnMut(Frame)) {
        let end = self.last;
        let mut pop_to = |frames: &mut Vec<(u32, usize)>, keep: usize| {
            while frames.len() > keep {
                let (f, first) = frames.pop().unwrap();
                pop(Frame { func: f, depth: frames.len() as u32, first, last: end });
            }
        };
        if n == 0 {
            self.frames.push((func, i));
        } else if self.frames.last().is_some_and(|&(f, _)| f == func) {
            // Still in the same function.
        } else if let Some(k) = self.frames.iter().rposition(|&(f, _)| f == func) {
            pop_to(&mut self.frames, k + 1);
        } else {
            pop_to(&mut self.frames, 0);
            self.frames.push((func, i));
        }
        self.last = i;
    }

    /// Function `func` returned (an exit record at `i`).
    fn exit(&mut self, func: u32, i: usize, pop: &mut impl FnMut(Frame)) {
        if let Some(k) = self.frames.iter().rposition(|&(f, _)| f == func) {
            while self.frames.len() > k {
                let (f, first) = self.frames.pop().unwrap();
                pop(Frame { func: f, depth: self.frames.len() as u32, first, last: i });
            }
        }
        self.last = i;
    }
}

/// The calls in records `from..=to` (of task `task`, or any), at least
/// `min` records long (smaller ones are too small to draw). `tasks[i]` is
/// the task running at record `i`.
pub fn frames(records: &[Record], tasks: &[u32], from: usize, to: usize, task: Option<u32>,
              min: usize) -> Vec<Frame> {
    let mut out = Vec::new();
    let to = to.min(records.len().saturating_sub(1));
    let mut stacks: BTreeMap<u32, Stack> = BTreeMap::new();
    let mut keep = |f: Frame| {
        if f.last >= from && f.first <= to && f.last - f.first + 1 >= min {
            out.push(Frame { first: f.first.max(from), ..f });
        }
    };
    for (i, r) in records.iter().enumerate().take(to + 1) {
        let t = tasks[i];
        if task.is_some_and(|x| x != t) {
            continue;
        }
        match *r {
            Record::Block { func, n } => stacks.entry(t).or_default().block(func, n, i, &mut keep),
            Record::Exit { func, .. } | Record::Return { func } => stacks.entry(t).or_default().exit(func, i, &mut keep),
            Record::Enter { .. } | Record::Line { .. } | Record::Task { .. } => {}
        }
    }
    // The calls still in progress at `to`.
    for s in stacks.values_mut() {
        let end = s.last;
        while let Some((f, first)) = s.frames.pop() {
            keep(Frame { func: f, depth: s.frames.len() as u32, first, last: end });
        }
    }
    out.sort_by_key(|f| (f.first, f.depth));
    out
}

/// A source line the kernel ran: at `record` (the block's, or the return
/// after which the block goes on), in `func` (`inlined` from another
/// function, maybe), `depth` calls deep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineStep {
    pub record: usize,
    pub func: u32,
    pub depth: u32,
    pub loc: crate::tables::Loc,
    pub inlined: Option<u32>,
}

/// The source lines run in records `from..=to` (of task `task`, or any), in
/// order: each block's lines up to its first call, then, as each call
/// returns, the lines up to the next (Tables::segments). The same line twice
/// in a row counts once.
pub fn lines(t: &crate::tables::Tables, records: &[Record], tasks: &[u32], from: usize, to: usize,
             task: Option<u32>) -> Vec<LineStep> {
    // Per task: the frames, each with its block and the stretch it is at.
    struct At {
        stack: Stack,
        blocks: Vec<(u32, usize)>, // per frame: (block, next stretch)
    }
    let mut out: Vec<LineStep> = Vec::new();
    let to = to.min(records.len().saturating_sub(1));
    let mut tasks_at: BTreeMap<u32, At> = BTreeMap::new();
    let mut emit = |out: &mut Vec<LineStep>, func: u32, n: u32, seg: usize, depth: u32, i: usize| {
        if i < from {
            return;
        }
        for s in t.segments(func, n).get(seg).into_iter().flatten() {
            let last = out.last();
            if last.is_some_and(|l| l.loc.file == s.loc.file && l.loc.line == s.loc.line && l.inlined == s.inlined) {
                continue;
            }
            out.push(LineStep { record: i, func, depth, loc: s.loc, inlined: s.inlined });
        }
    };
    for (i, r) in records.iter().enumerate().take(to + 1) {
        let tk = tasks[i];
        if task.is_some_and(|x| x != tk) {
            continue;
        }
        let at = tasks_at.entry(tk).or_insert_with(|| At { stack: Stack::default(), blocks: Vec::new() });
        match *r {
            Record::Block { func, n } => {
                at.stack.block(func, n, i, &mut |_| {});
                let depth = at.stack.frames.len();
                at.blocks.resize(depth, (u32::MAX, 0));
                at.blocks[depth - 1] = (n, 1);
                emit(&mut out, func, n, 0, depth as u32 - 1, i);
            }
            Record::Exit { func, .. } | Record::Return { func } => {
                at.stack.exit(func, i, &mut |_| {});
                let depth = at.stack.frames.len();
                at.blocks.truncate(depth);
                // The caller goes on after the call: its block's next stretch.
                if let (Some(&(f, _)), Some((n, seg))) = (at.stack.frames.last(), at.blocks.last_mut()) {
                    if *n != u32::MAX {
                        let (block, stretch) = (*n, *seg);
                        *seg += 1;
                        emit(&mut out, f, block, stretch, depth as u32 - 1, i);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// The call stack of the task running at record `at`, outermost first.
pub fn stack_at(records: &[Record], tasks: &[u32], at: usize) -> Vec<u32> {
    let Some(&task) = tasks.get(at) else { return Vec::new() };
    let mut s = Stack::default();
    for (i, r) in records.iter().enumerate().take(at + 1) {
        if tasks[i] == task {
            match *r {
                Record::Block { func, n } => s.block(func, n, i, &mut |_| {}),
                Record::Exit { func, .. } | Record::Return { func } => s.exit(func, i, &mut |_| {}),
                Record::Enter { .. } | Record::Line { .. } | Record::Task { .. } => {}
            }
        }
    }
    s.frames.into_iter().map(|(f, _)| f).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(func: u32, n: u32) -> Record {
        Record::Block { func, n }
    }

    #[test]
    fn calls_nest_and_return() {
        // f calls g, g calls h; back in g, back in f; then f calls g again.
        let r = [b(1, 0), b(1, 1), b(2, 0), b(3, 0), b(2, 1), b(1, 2), b(2, 0), b(1, 3)];
        let t = [7; 8];
        let f = frames(&r, &t, 0, 7, None, 1);
        let got: Vec<_> = f.iter().map(|f| (f.func, f.depth, f.first, f.last)).collect();
        assert_eq!(got, [(1, 0, 0, 7), (2, 1, 2, 4), (3, 2, 3, 3), (2, 1, 6, 6)]);
        assert_eq!(stack_at(&r, &t, 3), [1, 2, 3]);
        assert_eq!(stack_at(&r, &t, 5), [1]);
    }

    #[test]
    fn lines_go_on_after_calls() {
        // f's block 0: lines 1, 2, a call (g: line 10), then line 3.
        let t = crate::tables::Tables::parse(
            "U\nP\t0\tf.c\nP\t1\tg.c\nB\t1\t0\t0:1:1\nL\t1\t0\t0:1;:2;|;:3\nB\t2\t0\t1:10:1\n", "");
        let r = [b(1, 0), b(2, 0), Record::Return { func: 2 }, Record::Return { func: 1 }];
        let got: Vec<_> = lines(&t, &r, &[7; 4], 0, 3, None).iter().map(|s| (s.loc.line, s.record, s.depth)).collect();
        assert_eq!(got, [(1, 0, 0), (2, 0, 0), (10, 1, 1), (3, 2, 0)]);
    }

    #[test]
    fn exits_pop() {
        // A syscall (1) that calls 2, then returns to user space; another.
        let exit = Record::Exit { func: 1, time: 0, current: 7, value: 0 };
        let r = [b(1, 0), b(2, 0), exit, b(1, 0)];
        assert_eq!(stack_at(&r, &[7; 4], 3), [1]);
    }

    #[test]
    fn tasks_have_their_own_stacks() {
        // Task 7 is in f when task 8 runs g; back on 7, f goes on.
        let r = [b(1, 0), b(2, 0), b(2, 1), b(1, 1)];
        let t = [7, 8, 8, 7];
        assert_eq!(stack_at(&r, &t, 3), [1]);
        let f = frames(&r, &t, 0, 3, Some(7), 1);
        assert_eq!(f, [Frame { func: 1, depth: 0, first: 0, last: 3 }]);
    }

    #[test]
    fn a_return_past_the_start_restarts_the_stack() {
        let r = [b(2, 3), b(3, 0), b(1, 4)];
        let f = frames(&r, &[7; 3], 0, 2, None, 1);
        let got: Vec<_> = f.iter().map(|f| (f.func, f.depth)).collect();
        assert_eq!(got, [(2, 0), (3, 1), (1, 0)]);
    }
}
