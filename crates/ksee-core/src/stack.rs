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

/// The function a task starts in (arch/wasm/kernel/process.c).
pub const TASK_ENTRY: &str = "wasm_task_entry";

impl Stack {
    /// Apply block `(func, n)` at record `i`.
    fn block(&mut self, func: u32, n: u32, i: usize) {
        if n == 0 {
            if func == Tables::id(TASK_ENTRY) {
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
/// `lines` and `chart` share.
#[derive(Default)]
struct Walk {
    stack: Stack,
    /// Per frame: its block, and the stretch of the block's lines next.
    blocks: Vec<(u32, usize)>,
    /// Per frame: the last line it ran.
    at: Vec<Option<Step>>,
}

impl Walk {
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

/// A stack chart of records `from..=to` in `n` columns: for each column,
/// the task running at its first record and that task's call stack then,
/// outermost first.
pub fn chart(t: &Tables, records: &[Record], tasks: &[u32], from: usize, to: usize, n: usize) -> Vec<(u32, Vec<ChartCall>)> {
    let mut out = Vec::with_capacity(n);
    if records.is_empty() || n == 0 || from > to {
        return out;
    }
    let to = to.min(records.len() - 1);
    let span = to - from + 1;
    let mut walks: BTreeMap<u32, Walk> = BTreeMap::new();
    let mut col = 0;
    for (i, r) in records.iter().enumerate().take(to + 1) {
        let task = tasks[i];
        let w = walks.entry(task).or_default();
        w.apply(t, r, i);
        // (A column's first record, or several columns' when zoomed in.)
        while col < n && i >= from + col * span / n {
            let calls = w.stack.frames.iter().enumerate()
                .map(|(d, &(func, first))| ChartCall { func, first, step: w.at.get(d).copied().flatten() }).collect();
            out.push((task, calls));
            col += 1;
        }
    }
    out
}

/// A source line the kernel ran: at `record` (the block's, or the return
/// after which the block goes on), in `func` (`inlined` from other
/// functions, maybe: see Tables::inlines), `depth` calls deep.
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
        let Some((func, depth, steps)) = walks.entry(tk).or_default().apply(t, r, i) else { continue };
        if i < from {
            continue;
        }
        for s in steps {
            if out.last().is_some_and(|l| l.loc.file == s.loc.file && l.loc.line == s.loc.line && l.inlined == s.inlined) {
                continue;
            }
            out.push(LineStep { record: i, func, depth, loc: s.loc, inlined: s.inlined });
        }
    }
    out
}

/// A call on a stack: `func`, from record `first`, with its arguments (the
/// Args record after its entry block) and its locals' addresses (its
/// Locals record, once it has run), if the trace has them.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub func: u32,
    pub first: usize,
    pub args: Option<Vec<u32>>,
    pub locals: Option<Vec<u32>>,
}

/// The call stack of the task running at record `at`, outermost first.
pub fn calls_at(records: &[Record], tasks: &[u32], at: usize) -> Vec<Call> {
    let Some(&task) = tasks.get(at) else { return Vec::new() };
    let mut s = Stack::default();
    for (i, r) in records.iter().enumerate().take(at + 1) {
        if tasks[i] == task {
            match *r {
                Record::Block { func, n } => s.block(func, n, i),
                Record::Return { func, .. } => s.exit(func),
                _ => {}
            }
        }
    }
    s.frames.into_iter().map(|(func, first)| {
        let args = match records.get(first + 1) {
            Some(&Record::Args { args, n }) if matches!(records[first], Record::Block { n: 0, .. }) => Some(args[..n as usize].to_vec()),
            _ => None,
        };
        // (Soon after the entry, on the same task, if it got that far.)
        let locals = (first..records.len().min(at + 1).min(first + 256))
            .filter(|&i| tasks[i] == task)
            .find_map(|i| match records[i] {
                Record::Locals { func: f, addrs, n } if f == func => Some(addrs[..n as usize].to_vec()),
                _ => None,
            });
        Call { func, first, args, locals }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(func: u32, n: u32) -> Record {
        Record::Block { func, n }
    }

    fn stack_at(records: &[Record], tasks: &[u32], at: usize) -> Vec<u32> {
        calls_at(records, tasks, at).into_iter().map(|c| c.func).collect()
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
    fn a_new_task_starts_empty() {
        // A task that exited in f (never returning), then a new task at its address.
        let entry = Tables::id(TASK_ENTRY);
        let r = [b(entry, 0), b(1, 0), b(entry, 0), b(2, 0)];
        assert_eq!(stack_at(&r, &[7; 4], 3), [entry, 2]);
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
