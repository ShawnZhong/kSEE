//! The page's state: the snapshot (a new one after each call), and two
//! moments: the cursor (where a click or a step left it) and the one under
//! the pointer, over the chart. The page shows the pointer's, else the
//! cursor's; derived from it: its call's lines, its stack, its frame.

use std::rc::Rc;

use dioxus::prelude::*;
use ksee_core::view::{run_to, Chart, Frame, Line, SyscallSummary};

use crate::machine;

/// The chart's columns: moments across its window; and the fewest steps it shows.
const COLUMNS: usize = 400;
const MIN_SPAN: usize = 20;

/// A value computed once and shared (a memo's: compared by identity, not
/// by content).
pub struct Shared<T>(pub Rc<T>);

impl<T> Clone for Shared<T> {
    fn clone(&self) -> Self {
        Shared(self.0.clone())
    }
}

impl<T> PartialEq for Shared<T> {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl<T> std::ops::Deref for Shared<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

/// A moment: a call (an index in the syscalls), a line it ran (an index
/// in its lines), and the frame chosen on the stack there, by its call's
/// first record and function (none: the innermost).
#[derive(Clone, PartialEq, Default)]
pub struct Moment {
    pub call: usize,
    pub line: usize,
    pub frame: Option<(usize, String)>,
}

/// What the state makes, computed once each time what it depends on
/// changes: the moment shown (the pointer's, else the cursor's), its call
/// (as the snapshot has it) and lines; the chart's window and the chart;
/// its line, stack, and the frame shown on it; and where the cursor is, in
/// steps (the chart's line: its line's share of
/// its block's step).
#[derive(Clone, Copy)]
pub struct Derived {
    pub shown: Memo<Option<Moment>>,
    pub syscall: Memo<Option<SyscallSummary>>,
    pub lines: Memo<Shared<Vec<Line>>>,
    pub window: Memo<(usize, usize)>,
    pub chart: Memo<Shared<Chart>>,
    pub line: Memo<Option<Line>>,
    pub stack: Memo<Shared<Vec<Frame>>>,
    pub frame: Memo<usize>,
    pub cursor: Memo<Option<f64>>,
    pub preview: Memo<Option<f64>>,
}

/// A moment's position in recording steps. Lines in one block divide the
/// block's step range evenly, so preview and committed cursors agree.
fn moment_step(at: &Moment, lines: &[Line]) -> Option<f64> {
    let i = lines.get(at.line)?.i;
    let (k, n) = (lines[..at.line].iter().rev().take_while(|l| l.i == i).count(), lines.iter().filter(|l| l.i == i).count());
    let (first, last) = machine().view(|v| (v.rec.blocks.partition_point(|&b| b < i), v.rec.step_of(i)));
    let first = first.min(last);
    Some(first as f64 + (last + 1 - first) as f64 * k as f64 / n as f64)
}

impl Derived {
    /// The memos (a component's hooks: the App's).
    pub fn new(ui: Ui) -> Derived {
        let shown = use_memo(move || (ui.peek)().or((ui.at)()));
        let syscall = use_memo(move || {
            (ui.snapshot)();
            if !matches!((ui.kernel)(), Some(Ok(()))) {
                return None;
            }
            let k = shown()?.call;
            machine().view(|v| v.syscalls().into_iter().nth(k))
        });
        let lines = use_memo(move || {
            (ui.snapshot)();
            Shared(Rc::new(syscall.cloned().map_or_else(Vec::new, |s| {
                let m = machine();
                m.view(|v| v.lines(s.first, s.last.unwrap_or(m.records().saturating_sub(1)), s.task.addr))
            })))
        });
        // (The chart's: none before the kernel is up.)
        let up = move || matches!((ui.kernel)(), Some(Ok(())));
        let window = use_memo(move || {
            (ui.snapshot)();
            let end = if up() { machine().view(|v| v.rec.steps()).saturating_sub(1) } else { 0 };
            (ui.win)().unwrap_or((0, end))
        });
        let chart = use_memo(move || {
            (ui.snapshot)();
            let (a, b) = window();
            Shared(Rc::new(if up() { machine().view(|v| v.chart(a, b, COLUMNS)) } else { Chart::default() }))
        });
        let line = use_memo(move || lines.read().get(shown()?.line).cloned());
        // (Its values as they are just before the line runs: at its own
        // record, past what the lines before it in its stretch did.)
        let stack = use_memo(move || {
            (ui.snapshot)();
            Shared(Rc::new(line.cloned().map_or_else(Vec::new, |l| {
                machine().at_moment(l.at, |v, mem| v.call_stack(l.i, Some(l.nth as usize), l.cut as usize, mem, l.at))
            })))
        });
        let frame = use_memo(move || shown().and_then(|m| m.frame).and_then(|(first, func)| stack.read().iter().position(|f| f.first == first && f.func == func)).unwrap_or(0));
        let cursor = use_memo(move || {
            (ui.snapshot)();
            let at = (ui.at)()?;
            if shown().is_some_and(|s| s.call == at.call) { moment_step(&at, &lines.read()) }
            else { moment_step(&at, &Ui::lines_of(at.call)) }
        });
        let preview = use_memo(move || {
            (ui.snapshot)();
            let at = (ui.peek)()?;
            if shown().is_some_and(|s| s.call == at.call) { moment_step(&at, &lines.read()) }
            else { moment_step(&at, &Ui::lines_of(at.call)) }
        });
        Derived { shown, syscall, lines, window, chart, line, stack, frame, cursor, preview }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Tab {
    Stack,
    Objects,
    Tasks,
    Console,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct RunId(u64);


#[derive(Clone, Copy)]
pub struct Ui {
    /// The published recording snapshot: what reads the recording subscribes
    /// to this identity and reads it again when it changes.
    pub snapshot: Signal<u64>,
    /// The newest requested program execution. Async results from older runs
    /// are ignored even if their underlying browser work finishes later.
    run: Signal<RunId>,
    /// The cursor (none: no call selected), and the moment under the
    /// pointer, if it is over the chart.
    pub at: Signal<Option<Moment>>,
    pub peek: Signal<Option<Moment>>,
    /// The chart's window (steps, both ends in; none: the whole recording).
    pub win: Signal<Option<(usize, usize)>>,
    pub tab: Signal<Tab>,
    /// The kernel: booting (none), up, or why it could not start.
    pub kernel: Signal<Option<Result<(), String>>>,
    pub console: Signal<String>,
    /// What the source shows instead of the cursor's place, until it
    /// moves: a definition (file, line).
    pub seen: Signal<Option<(String, u32)>>,
    /// The text looked for in the functions' names (the chart's bars).
    pub find: Signal<String>,
    /// The lesson open (an index in the examples), the program made for
    /// it (its own, or a try), and how each of its lines went.
    pub lesson: Signal<usize>,
    pub program: Signal<Vec<crate::lesson::Step>>,
    pub outcomes: Signal<Vec<crate::lesson::Outcome>>,
}

impl Ui {
    pub fn new() -> Ui {
        Ui {
            snapshot: Signal::new(0),
            run: Signal::new(RunId::default()),
            at: Signal::new(None),
            peek: Signal::new(None),
            win: Signal::new(None),
            tab: Signal::new(Tab::Stack),
            kernel: Signal::new(None),
            console: Signal::new(String::new()),
            seen: Signal::new(None),
            find: Signal::new(String::new()),
            lesson: Signal::new(0),
            program: Signal::new(crate::examples().first().map(|e| e.steps.clone()).unwrap_or_default()),
            outcomes: Signal::new(Vec::new()),
        }
    }

    /// Begin a new program execution and invalidate all older publishers.
    pub fn begin_run(mut self) -> RunId {
        let next = RunId(self.run.peek().0.wrapping_add(1));
        self.run.set(next);
        self.outcomes.write().clear();
        self.console.set(String::new());
        self.at.set(None);
        self.peek.set(None);
        self.seen.set(None);
        next
    }

    pub fn is_current(&self, run: RunId) -> bool {
        *self.run.peek() == run
    }

    pub fn publish_outcomes(mut self, run: RunId, outcomes: Vec<crate::lesson::Outcome>) -> bool {
        if !self.is_current(run) {
            return false;
        }
        self.outcomes.set(outcomes);
        true
    }

    /// Publish the current execution state after a new Machine snapshot.
    pub fn publish_snapshot(mut self, run: RunId, outcomes: Vec<crate::lesson::Outcome>, console: String) -> bool {
        if !self.is_current(run) {
            return false;
        }
        self.outcomes.set(outcomes);
        self.console.write().push_str(&console);
        self.snapshot += 1;
        true
    }

    pub fn open_lesson(mut self, lesson: usize) {
        let examples = crate::examples();
        let Some(example) = examples.get(lesson) else { return };
        self.lesson.set(lesson);
        self.program.set(example.steps.clone());
    }

    pub fn restore_lesson(mut self, lesson: usize, program: Vec<crate::lesson::Step>) {
        if lesson >= crate::examples().len() {
            return;
        }
        self.lesson.set(lesson);
        self.program.set(program);
    }

    pub fn replace_program(mut self, program: Vec<crate::lesson::Step>) {
        self.program.set(program);
    }

    pub fn edit_program(mut self, edit: impl FnOnce(&mut Vec<crate::lesson::Step>)) {
        edit(&mut self.program.write());
    }

    pub fn commit(mut self, moment: Option<Moment>) {
        self.peek.set(None);
        self.seen.set(None);
        self.at.set(moment);
    }

    pub fn set_preview(mut self, moment: Option<Moment>) {
        if *self.peek.peek() != moment {
            self.peek.set(moment);
        }
    }

    pub fn set_window(mut self, window: Option<(usize, usize)>) {
        self.win.set(window);
    }

    pub fn set_tab(mut self, tab: Tab) {
        self.tab.set(tab);
    }

    pub fn show_definition(mut self, definition: Option<(String, u32)>) {
        self.seen.set(definition);
    }

    pub fn set_function_filter(mut self, filter: String) {
        self.find.set(filter);
    }

    /// The call selected (the cursor's).
    pub fn call(&self) -> Option<usize> {
        self.at.read().as_ref().map(|m| m.call)
    }

    /// Syscall `k`'s lines (its task's), as the snapshot has them.
    pub fn lines_of(k: usize) -> Vec<Line> {
        let m = machine();
        m.view(|v| {
            let Some(s) = v.syscalls().into_iter().nth(k) else { return Vec::new() };
            v.lines(s.first, s.last.unwrap_or(m.records().saturating_sub(1)), s.task.addr)
        })
    }

    /// Select call `k` (an index in the syscalls), the cursor at its own
    /// code's first line (past the syscall entry's), on its innermost
    /// frame; the chart's window the call.
    pub fn select(self, k: usize) {
        self.zoom(k);
        let own = Ui::lines_of(k).iter().position(|l| l.func != ksee_core::arch::SYSCALL);
        self.commit(Some(Moment { call: k, line: own.unwrap_or(0), frame: None }));
    }

    /// The chart's window: call `k`, with a little room either side (its
    /// lines a step or so apart, not many to a column).
    pub fn zoom(self, k: usize) {
        let Some((a, b)) = machine().view(|v| {
            let s = v.syscalls().into_iter().nth(k)?;
            Some((v.rec.step_of(s.first), v.rec.step_of(s.last.unwrap_or(v.rec.len().saturating_sub(1)))))
        }) else { return };
        let room = ((b - a) / 50).max(4);
        self.view(a as f64 - room as f64, b - a + 2 * room);
    }

    /// The chart's window: `span` steps from `start`, kept in the recording
    /// (as wide, at its ends).
    pub fn view(self, start: f64, span: usize) {
        let total = machine().view(|v| v.rec.steps()).max(1);
        let span = span.max(MIN_SPAN).min(total - 1);
        let a = (start.max(0.0) as usize).min(total - 1 - span);
        self.set_window(Some((a, a + span)));
    }

    /// The cursor to line `k` of its call, on its innermost frame.
    pub fn go(self, k: usize) {
        if let Some(call) = self.call() {
            self.commit(Some(Moment { call, line: k, frame: None }));
        }
    }

    /// The cursor at the next (`dir` 1) or previous (`dir` -1) run of
    /// `file`'s `line`, within its call.
    pub fn run_to(self, file: &str, line: u32, dir: isize) {
        let Some(at) = self.at.peek().clone() else { return };
        let lines = Ui::lines_of(at.call);
        if let Some(k) = run_to(&lines, at.line, file, line, dir) {
            self.go(k);
        }
    }

    /// The moment at record `i` (in the chart: its `nth` line), on frame `frame` there:
    /// the call it is in, its line there (the last at or before it).
    pub fn moment_at(&self, i: usize, nth: Option<usize>, frame: Option<(usize, String)>, shown: Option<usize>, shown_lines: &[Line]) -> Option<Moment> {
        let (call, own) = machine().view(|v| (v.rec.syscall_at(i), v.line_of(i).or(nth.map(|n| (i, n as u32)))));
        let call = call?;
        // (A write, a comparison: its own line; a block: its first line; else the last before.)
        let line = |lines: &[Line]| match own {
            Some((r, nth)) => lines.iter().rposition(|l| l.i == r && l.nth <= nth),
            None => lines.iter().position(|l| l.i == i),
        }.or_else(|| lines.iter().rposition(|l| l.i <= i)).unwrap_or(0);
        let line = if shown == Some(call) { line(shown_lines) } else { line(&Ui::lines_of(call)) };
        Some(Moment { call, line, frame })
    }

    /// The cursor to record `i`, on frame `frame` (see `moment_at`).
    pub fn go_record(self, d: Derived, i: usize, frame: Option<(usize, String)>) {
        let shown = d.shown.peek().as_ref().map(|m| m.call);
        if let Some(m) = self.moment_at(i, None, frame, shown, &d.lines.read()) {
            self.commit(Some(m));
        }
    }
}
