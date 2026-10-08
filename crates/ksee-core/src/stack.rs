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
//! its task_struct. So does a syscall (wasm_syscall): it is entered from
//! the task's program, with nothing of the kernel under it (a syscall
//! before it may have returned unrecorded: a runner's wait).

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

/// Where a call is: in block `n` (NONE: not known), running stretch `seg`
/// of the block's lines (tables::Block) from `record` (the block's, or the
/// return into it).
#[derive(Debug, Clone, Copy)]
struct Place {
    call: u32,
    n: u32,
    seg: u32,
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
    /// The Var records (the values of variables kept in wasm locals), by
    /// the call they are in: (call, record), in order.
    vars: Vec<(u32, u32)>,
}

impl Calls {
    pub fn new(t: &Tables, records: &[Record], tasks: &[u32]) -> Calls {
        let mut c = Calls { calls: Vec::new(), at: Vec::with_capacity(records.len()), places: Vec::new(), vars: Vec::new() };
        // (Per task: its calls on the stack, outermost first, and where the innermost is.)
        let mut stacks: BTreeMap<u32, (Vec<u32>, u32)> = BTreeMap::new();
        let starts = [t.find(crate::arch::TASK_ENTRY), t.find(crate::arch::SYSCALL)];
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
                            (0, _) if starts.contains(&Some(func)) => 0,
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
                    *place = c.place(Place { call, n, seg: 0, record: i as u32 });
                }
                Record::Return { func, .. } => {
                    if let Some(k) = frames.iter().rposition(|&k| c.calls[k as usize].func == func) {
                        // (The caller goes on after the call: its block's next stretch.)
                        let site = c.calls[frames[k] as usize].site;
                        c.end(frames, k, i);
                        *place = match frames.last() {
                            Some(&call) if site != NONE && c.places[site as usize].call == call => {
                                let p = c.places[site as usize];
                                c.place(Place { call, n: p.n, seg: p.seg + 1, record: i as u32 })
                            }
                            Some(&call) => c.place(Place { call, n: NONE, seg: 0, record: i as u32 }),
                            None => NONE,
                        };
                    }
                }
                Record::Var { .. } => {
                    if let Some(&call) = frames.last() {
                        c.vars.push((call, i as u32));
                    }
                }
                _ => {}
            }
            c.at.push(*place);
        }
        c.vars.sort_unstable();
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

    /// The line place `p`'s call is at (the last it has run in its block).
    fn line(&self, t: &Tables, p: &Place) -> Option<Step> {
        t.block(self.calls[p.call as usize].func, p.n)?.last_by(p.seg)
    }

    /// The lines record `i` runs, if it runs some (a block, or a return
    /// into the middle of one): (its place, the lines).
    fn stretch<'t>(&self, t: &'t Tables, i: usize) -> Option<(&Place, &'t [Step])> {
        let p = self.place_at(i).filter(|p| p.record as usize == i && p.n != NONE)?;
        Some((p, t.stretch(self.calls[p.call as usize].func, p.n, p.seg)))
    }

    /// The call running at record `i` (its task's innermost).
    pub(crate) fn call_at(&self, i: usize) -> Option<&Call> {
        self.calls.get(self.place_at(i)?.call as usize)
    }

    /// The Var records of the call from record `first` (a block 0), by
    /// record `upto`, in order.
    pub(crate) fn vars_of(&self, first: usize, upto: usize) -> impl Iterator<Item = usize> + '_ {
        let call = self.call_index(first).unwrap_or(NONE);
        let from = self.vars.partition_point(|&(c, _)| c < call);
        self.vars[from..].iter().take_while(move |&&(c, i)| c == call && i as usize <= upto).map(|&(_, i)| i as usize)
    }

    /// The call the block 0 at record `first` begins.
    pub(crate) fn call_from(&self, first: usize) -> Option<&Call> {
        self.calls.get(self.call_index(first)? as usize)
    }

    /// The index of the call the block 0 at record `first` begins.
    fn call_index(&self, first: usize) -> Option<u32> {
        let call = self.place_at(first)?.call;
        (self.calls.get(call as usize)?.first == first).then_some(call)
    }

    /// Where the call from record `first` (a block 0) ends: the record
    /// that takes it off its task's stack (its return), or None if it has
    /// not ended when the records do.
    pub fn call_end(&self, first: usize) -> Option<usize> {
        self.call_from(first)?.last
    }

    /// The call stack of the task running at record `i`, outermost first,
    /// each call at its last line; the innermost at the first of the lines
    /// record `i` runs if it starts them (a block, a return into one), or,
    /// if `nth`, at the nth (a LineStep's).
    pub(crate) fn stack_at(&self, t: &Tables, i: usize, nth: Option<usize>) -> Vec<StackCall> {
        let Some((mut k, inner)) = self.innermost(t, i, nth) else { return Vec::new() };
        let mut out = alloc::vec![inner];
        while let Some((parent, at)) = self.caller(t, k) {
            out.push(at);
            k = parent;
        }
        out.reverse();
        out
    }

    /// The innermost call of the stack at record `i` (see stack_at): its
    /// index in `calls`, and it at its line.
    pub(crate) fn innermost(&self, t: &Tables, i: usize, nth: Option<usize>) -> Option<(u32, StackCall)> {
        let p = self.place_at(i)?;
        // (At the record that starts a stretch, its task is at the
        // stretch's first line, or the `nth`; after, at its last.)
        let step = self.stretch(t, i).and_then(|(_, s)| s.get(nth.unwrap_or(0)).copied()).or_else(|| self.line(t, p));
        let c = &self.calls[p.call as usize];
        Some((p.call, StackCall { func: c.func, first: c.first, step }))
    }

    /// Call `k`'s caller: its index, and it at its line calling `k`.
    pub(crate) fn caller(&self, t: &Tables, k: u32) -> Option<(u32, StackCall)> {
        let c = &self.calls[k as usize];
        let parent = c.parent?;
        let p = &self.calls[parent as usize];
        Some((parent, StackCall { func: p.func, first: p.first, step: self.places.get(c.site as usize).and_then(|s| self.line(t, s)) }))
    }

    /// The line the store (or comparison, or variable set) at record `i` is on (its record's
    /// line: which of its block's lines), in the call running it.
    pub(crate) fn line_at(&self, t: &Tables, i: usize, records: &[Record]) -> Option<LineStep> {
        let line = records.get(i)?.line()?;
        let p = self.place_at(i)?;
        let c = &self.calls[p.call as usize];
        let b = t.block(c.func, p.n)?;
        // (None, or a line of another stretch than the one it runs in (one
        // before a call it made): the store has no line of its own (the
        // compiler gave it none), so the line its stretch is on, its last.)
        let (s, nth) = match b.stretch_of(line) {
            Some(seg) if seg == p.seg => (b.steps[line as usize], line - b.start(seg) as u32),
            _ => {
                let steps = b.stretch(p.seg);
                (*steps.last()?, steps.len() as u32 - 1)
            }
        };
        Some(LineStep { record: p.record as usize, at: i, nth, index: line, func: c.func, call: p.call, depth: c.depth, loc: s.loc, inlined: s.inlined, cut: 0, run: 0, callee: None })
    }

    /// The source lines task `task` ran in records `from..=to`, in order: each block's lines up to its first call, then, as
    /// each call returns, the lines up to the next (tables::Block). The
    /// same line twice in a row counts once.
    pub(crate) fn lines(&self, t: &Tables, records: &[Record], tasks: &[u32], from: usize, to: usize,
                 task: u32) -> Vec<LineStep> {
        let mut out: Vec<LineStep> = Vec::new();
        // (How often each call entered each block so far: in a call, a block
        // runs again only in a loop, so its lines' turn.)
        let mut runs: BTreeMap<(u32, u32), u32> = BTreeMap::new();
        for i in from..=to.min(records.len().saturating_sub(1)) {
            if tasks[i] != task {
                continue;
            }
            let Some((p, steps)) = self.stretch(t, i) else { continue };
            let c = &self.calls[p.call as usize];
            // (After a return into it: the line that made the call says what
            // it returned, the value its `ret = f(...)` got; nothing, a void
            // function's.)
            if let Record::Return { func, value: Some(value) } = records[i]
                && let Some(l) = out.iter_mut().rev().find(|l| l.depth <= c.depth).filter(|l| l.depth == c.depth && l.func == c.func) {
                l.callee = Some((func, value, i));
            }
            let run = runs.entry((p.call, p.n)).or_default();
            if p.seg == 0 {
                *run += 1;
            }
            let run = *run;
            // (The stretch's lines are numbered on from the block's earlier
            // ones: a store says which it is on; the first store of each line,
            // among the records up to the task's next block or return.)
            let base = t.block(c.func, p.n).map_or(0, |b| b.start(p.seg));
            let mut first_at: BTreeMap<u32, usize> = BTreeMap::new();
            let mut last = i;
            for (j, r) in records[i + 1..].iter().enumerate().map(|(k, r)| (i + 1 + k, r)) {
                if tasks[j] != task {
                    continue;
                }
                // (A line is at its first store, or comparison: the
                // instruction that does what it says, its assignment or its
                // test, as the debug info's key instructions have it.)
                let Some(line) = r.line() else { break };
                first_at.entry(line).or_insert(j);
                last = j;
            }
            for (nth, s) in steps.iter().enumerate() {
                // (Code with no line, the compiler's own: no step.)
                if s.loc.line == 0 {
                    continue;
                }
                // (Where it is: just before it runs, memory as the lines
                // before it left it: the record before the first on it or a
                // later one; with none, after the stretch's last.)
                let at = first_at.range((base + nth) as u32..).map(|(_, &j)| j).min().map_or(last, |j| j - 1).max(i);
                let step = LineStep { record: i, at, nth: nth as u32, index: (base + nth) as u32, func: c.func, call: p.call, depth: c.depth, loc: s.loc, inlined: s.inlined, cut: 0, run, callee: None };
                // (Code inlined it enters, as a call would be: first the line
                // calling each function it enters, in the one calling it, the
                // outermost first; from this call's line before, or, the
                // call just entered, from none.)
                let before = match out.iter().rev().find(|l| l.depth <= c.depth) {
                    Some(l) if (l.func, l.depth) == (c.func, c.depth) => &t.chain(l.inlined)[l.cut as usize..],
                    _ => &[][..],
                };
                let into = t.chain(s.inlined);
                let same = into.iter().rev().zip(before.iter().rev()).take_while(|(a, b)| a == b).count();
                for level in (0..into.len() - same).rev() {
                    let (loc, outer) = (into[level].1, &into[level + 1..]);
                    // (A site with no line, the compiler's own: no step, as for lines.)
                    if loc.line != 0 && !out.last().is_some_and(|l| l.loc == loc && &t.chain(l.inlined)[l.cut as usize..] == outer) {
                        out.push(LineStep { loc, cut: level as u32 + 1, ..step });
                    }
                }
                if out.last().is_some_and(|l| l.loc == s.loc && (l.inlined, l.cut) == (s.inlined, 0)) {
                    continue;
                }
                out.push(step);
            }
        }
        out
    }
}

/// A call on a stack (the Stack's, the chart's): `func` from record `first`, at `step` (its last
/// line then, if known: with the functions it is inlined from).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct StackCall {
    pub func: u32,
    pub first: usize,
    pub step: Option<Step>,
}

/// A source line the kernel ran: at `record` (the block's, or the return
/// after which the block goes on), the `nth` of the lines it runs there, in
/// `func` (`inlined` from other functions, maybe: see Tables::inlines),
/// `depth` calls deep. The line is `at` the record just before it runs
/// (before the first record on it or on a later line of its stretch, a
/// store, a comparison, a variable set: Record::Store's line; with none,
/// the stretch's last): where the cursor goes, and memory as it was there,
/// as a debugger stops before a line. `run`:
/// the how-manieth time its call enters its block (in the lines asked
/// for; 0 if not counted): a block runs again in a call only in a loop,
/// so a loop's turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineStep {
    pub record: usize,
    pub at: usize,
    pub nth: u32,
    /// Which of its block's lines it is (a Store's, a Compare's or a Var's
    /// line: Record::line).
    pub index: u32,
    pub func: u32,
    /// Its call (an index in Calls::calls), `depth` calls deep.
    pub call: u32,
    pub depth: u32,
    pub loc: crate::tables::Loc,
    pub inlined: Option<u32>,
    /// A line calling code inlined (`inlined`'s), not yet in it: how many
    /// of its inline chain's functions, innermost first, it is not in.
    pub cut: u32,
    pub run: u32,
    /// A line that made a call: the call's function, what it returned,
    /// and where (its Return record).
    pub callee: Option<(u32, i64, usize)>,
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

    fn lines(t: &Tables, records: &[Record], tasks: &[u32], from: usize, to: usize, task: u32) -> Vec<LineStep> {
        Calls::new(t, records, tasks).lines(t, records, tasks, from, to, task)
    }

    fn call_end(records: &[Record], tasks: &[u32], first: usize) -> Option<usize> {
        Calls::new(&Tables::default(), records, tasks).call_end(first)
    }

    #[test]
    fn lines_of_a_long_trace() {
        // f's block 1 (lines 5, 6) 2000 times, each with its stores: the
        // lines, each turn counted; a task switch in the middle names no
        // lines of its own.
        let t = crate::tables::Tables::parse("U\nP\t0\tf.c\nL\t1\t0\t0:1\nL\t1\t1\t0:5;:6\n", "");
        let st = |line| Record::Store { addr: 8, size: 4, old: 0, line, from: None };
        let mut r = Vec::new();
        let mut tasks = Vec::new();
        r.push(b(1, 0));
        tasks.push(7);
        for k in 0..2000 {
            r.push(b(1, 1));
            tasks.push(7);
            r.push(st(0));
            tasks.push(7);
            r.push(st(1));
            tasks.push(7);
            if k == 1000 {
                r.push(b(9, 0));
                tasks.push(8);
            }
        }
        let got = lines(&t, &r, &tasks, 0, r.len() - 1, 7);
        assert_eq!(got.len(), 1 + 2000 * 2);
        assert_eq!(got[0].loc.line, 1);
        assert_eq!(got.last().map(|s| (s.loc.line, s.run)), Some((6, 2000)));
        assert!(got.iter().all(|s| s.at <= s.record + 2));
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
            "U\nP\t0\tf.c\nP\t1\tg.c\nL\t1\t0\t0:1;:2;|;:3\nL\t2\t0\t1:10\n", "");
        let r = [b(1, 0), b(2, 0), Record::Return { func: 2, value: Some(0) }, Record::Return { func: 1, value: Some(0) }];
        let got: Vec<_> = lines(&t, &r, &[7; 4], 0, 3, 7).iter().map(|s| (s.loc.line, s.record, s.depth)).collect();
        assert_eq!(got, [(1, 0, 0), (2, 0, 0), (10, 1, 1), (3, 2, 0)]);
        // f's line 2 made the call to g: it says g returned 7.
        let r = [b(1, 0), b(2, 0), Record::Return { func: 2, value: Some(7) }, Record::Return { func: 1, value: Some(9) }];
        let got = lines(&t, &r, &[7; 4], 0, 3, 7);
        assert_eq!(got.iter().map(|s| s.callee.map(|(f, v, _)| (f, v))).collect::<Vec<_>>(), [None, Some((2, 7)), None, None]);
        // A loop: f's block 1 (lines 5, 6) three times; each turn counted.
        let t = crate::tables::Tables::parse("U\nP\t0\tf.c\nL\t1\t0\t0:1\nL\t1\t1\t0:5;:6\n", "");
        let r = [b(1, 0), b(1, 1), b(1, 1), b(1, 1)];
        let got: Vec<_> = lines(&t, &r, &[7; 4], 0, 3, 7).iter().map(|s| (s.loc.line, s.run)).collect();
        assert_eq!(got, [(1, 1), (5, 1), (6, 1), (5, 2), (6, 2), (5, 3), (6, 3)]);
    }

    #[test]
    fn lines_are_just_before_they_run() {
        // f's block 0 runs lines 1, 2, 3, then calls g (line 10), then line 4;
        // line 2 stores twice, line 4 once: a line is at the record before
        // its first (line 2: the block), one with none after the lines
        // before it (line 3: after line 2's stores), a stretch's first line
        // at the block, or the return, itself.
        let t = crate::tables::Tables::parse(
            "U\nP\t0\tf.c\nP\t1\tg.c\nL\t1\t0\t0:1;:2;:3;|;:4\nL\t2\t0\t1:10\n", "");
        let st = |line| Record::Store { addr: 8, size: 4, old: 0, line, from: None };
        let r = [b(1, 0), st(1), st(1), b(2, 0), Record::Return { func: 2, value: Some(0) }, st(3), Record::Return { func: 1, value: Some(0) }];
        let got: Vec<_> = lines(&t, &r, &[7; 7], 0, 6, 7).iter().map(|s| (s.loc.line, s.record, s.at)).collect();
        assert_eq!(got, [(1, 0, 0), (2, 0, 0), (3, 0, 2), (10, 3, 3), (4, 4, 4)]);
        // A store is on its line (the block's third item, line 4, after the call); the
        // stack there has f at line 4, and at g, f where it calls g (line 3).
        let c = Calls::new(&t, &r, &[7; 7]);
        let at = |i| c.line_at(&t, i, &r).map(|s| (s.loc.line, s.nth, s.record));
        assert_eq!((at(1), at(5), at(0)), (Some((2, 1, 0)), Some((4, 0, 4)), None));
        let lines_of = |i| c.stack_at(&t, i, None).iter().map(|c| c.step.map(|s| s.loc.line)).collect::<Vec<_>>();
        assert_eq!((lines_of(3), lines_of(5)), (alloc::vec![Some(3), Some(10)], alloc::vec![Some(4)]));
        // (At the block's own record, f is at its first line, about to run it; after, at its last.)
        assert_eq!((lines_of(0), lines_of(2)), (alloc::vec![Some(1)], alloc::vec![Some(3)]));
    }

    #[test]
    fn a_store_with_no_line_of_its_own_is_on_its_stretchs() {
        // f's block 0 runs lines 1, 2, 3, then calls g, then line 4: a store
        // before the call whose line is past its stretch (4, after the call;
        // or none the block has) is where the stretch ends, line 3.
        let t = crate::tables::Tables::parse("U\nP\t0\tf.c\nL\t1\t0\t0:1;:2;:3;|;:4\n", "");
        let st = |line| Record::Store { addr: 8, size: 4, old: 0, line, from: None };
        let r = [b(1, 0), st(3), st(9)];
        let c = Calls::new(&t, &r, &[7; 3]);
        let at = |i| c.line_at(&t, i, &r).map(|s| (s.loc.line, s.nth));
        assert_eq!((at(1), at(2)), (Some((3, 2)), Some((3, 2))));
    }

    #[test]
    fn a_new_task_starts_empty() {
        // A task that exited in f (never returning), then a new task at its address.
        let t = Tables::parse("U\nP\t0\tf.c\nF\t5\twasm_task_entry\t0:1\t0\n", "");
        let r = [b(5, 0), b(1, 0), b(5, 0), b(2, 0)];
        let got: Vec<u32> = Calls::new(&t, &r, &[7; 4]).stack_at(&t, 3, None).into_iter().map(|c| c.func).collect();
        assert_eq!(got, [5, 2]);
        // A syscall (6) entered again, the one before (in 1) never seen returning: nothing under it.
        let t = Tables::parse("U\nP\t0\tf.c\nF\t6\twasm_syscall\t0:1\t0\n", "");
        let r = [b(6, 0), b(1, 0), b(6, 0), b(2, 0)];
        let got: Vec<u32> = Calls::new(&t, &r, &[7; 4]).stack_at(&t, 3, None).into_iter().map(|c| c.func).collect();
        assert_eq!(got, [6, 2]);
    }

    #[test]
    fn a_record_has_its_tasks_stack() {
        // Task 7's f (1) calls g (2); task 8 runs h (3); back on 7, g returns.
        // At records 1, 2, 4: 7's f and g, 8's h, 7's f.
        let t = Tables::default();
        let r = [b(1, 0), b(2, 0), b(3, 0), Record::Return { func: 2, value: Some(0) }, b(1, 1)];
        let c = Calls::new(&t, &r, &[7, 7, 8, 7, 7]);
        let got: Vec<_> = [1, 2, 4].iter().map(|&i| c.stack_at(&t, i, None).iter().map(|c| c.func).collect::<Vec<_>>()).collect();
        assert_eq!(got, [alloc::vec![1, 2], alloc::vec![3], alloc::vec![1]]);
    }

    #[test]
    fn calls_end_where_they_return() {
        // f (1) calls g (2), which returns; f goes on, then returns. (Task 8 runs between.)
        let r = [b(1, 0), b(2, 0), b(9, 0), Record::Return { func: 2, value: Some(0) }, b(1, 1), Record::Return { func: 1, value: Some(0) }, b(3, 0)];
        let t = [7, 7, 8, 7, 7, 7, 7];
        assert_eq!((call_end(&r, &t, 1), call_end(&r, &t, 0), call_end(&r, &t, 6)), (Some(3), Some(5), None));
    }

    #[test]
    fn exits_pop() {
        // A syscall (1) that calls 2, then returns to user space; another.
        let exit = Record::Return { func: 1, value: Some(0) };
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
