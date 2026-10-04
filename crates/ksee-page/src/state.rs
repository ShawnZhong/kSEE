//! The page's state: the snapshot (a new one after each call), and two
//! moments: the cursor (where a click or a step left it) and the one under
//! the pointer, over the chart. The page shows the pointer's, else the
//! cursor's; derived from it: its call's lines, its stack, its frame.

use std::rc::Rc;

use dioxus::prelude::*;
use ksee_core::view::{run_to, Chart, Frame, Line, SyscallSummary};

use crate::machine;

/// The chart's columns: moments across its window; and the fewest steps it shows.
const COLUMNS: usize = 1200;
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
/// in its lines), and the level chosen on the stack there, from the
/// outermost (0: the chart's bottom bar; none, or past the stack's top: the
/// innermost). A level, not a call: moving along, it stays.
#[derive(Clone, PartialEq, Default)]
pub struct Moment {
    pub call: usize,
    pub line: usize,
    pub frame: Option<usize>,
}

/// What the state makes, computed once each time what it depends on
/// changes: the moment shown (the pointer's, else the cursor's), its call
/// (as the snapshot has it) and lines; the chart's window and the chart;
/// its line, stack, and the frame shown on it; and where the cursor is, in
/// steps (its line's: the chart's line).
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

/// A moment's step: its line's (the steps are the lines stepping visits).
fn moment_step(at: &Moment, lines: &[Line]) -> Option<f64> {
    let i = lines.get(at.line)?.i;
    let k = lines[..at.line].iter().rev().take_while(|l| l.i == i).count();
    Some((machine().view(|v| v.rec.first_step(i)) + k) as f64)
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
            machine().view(|v| v.syscall(k))
        });
        let lines = use_memo(move || {
            (ui.snapshot)();
            Shared(Rc::new(syscall.cloned().map_or_else(Vec::new, |s| {
                machine().view(|v| v.lines(s.first, s.last, s.task.addr))
            })))
        });
        // (The chart's: none before the kernel is up.)
        let up = move || matches!((ui.kernel)(), Some(Ok(())));
        let window = use_memo(move || {
            (ui.snapshot)();
            let end = if up() { machine().view(|v| v.rec.steps.len()).saturating_sub(1) } else { 0 };
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
                machine().view(|v| v.call_stack(l.i, Some(l.nth as usize), l.cut as usize, l.at))
            })))
        });
        let frame = use_memo(move || {
            let n = stack.read().len();
            shown().and_then(|m| m.frame).filter(|&l| l < n).map_or(0, |l| n - 1 - l)
        });
        let cursor = use_memo(move || {
            (ui.snapshot)();
            let at = (ui.at)()?;
            if shown().is_some_and(|s| s.call == at.call) { moment_step(&at, &lines.read()) }
            else { moment_step(&at, &Ui::lines_of(at.call)) }
        });
        // (The pointer's: the moment shown, its lines `lines`.)
        let preview = use_memo(move || moment_step(&(ui.peek)()?, &lines.read()));
        Derived { shown, syscall, lines, window, chart, line, stack, frame, cursor, preview }
    }
}

/// A panel: what a lesson's link opens too.
pub use ksee_core::text::LessonTab as Tab;

#[derive(Clone, Copy)]
pub struct Ui {
    /// The published recording snapshot: what reads the recording subscribes
    /// to this identity and reads it again when it changes.
    pub snapshot: Signal<u64>,
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
    pub outcomes: Signal<Vec<ksee_host::Outcome>>,
}

impl Ui {
    pub fn new() -> Ui {
        Ui {
            snapshot: Signal::new(0),
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

    /// A run begun: what the last made cleared.
    pub fn begin_run(mut self) {
        self.outcomes.write().clear();
        self.console.set(String::new());
        self.at.set(None);
        self.peek.set(None);
        self.seen.set(None);
    }

    /// Lesson `k` open, with `program` (its own, or a try): made on a
    /// fresh kernel.
    pub fn open(mut self, k: usize, program: Vec<crate::lesson::Step>) {
        self.lesson.set(k);
        self.program.set(program);
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

    /// The call selected (the cursor's).
    pub fn call(&self) -> Option<usize> {
        self.at.read().as_ref().map(|m| m.call)
    }

    /// Syscall `k`'s lines (its task's), as the snapshot has them.
    pub fn lines_of(k: usize) -> Vec<Line> {
        machine().view(|v| {
            let Some(s) = v.syscall(k) else { return Vec::new() };
            v.lines(s.first, s.last, s.task.addr)
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
            let s = v.syscall(k)?;
            Some((v.rec.step_of(s.first), v.rec.step_of(s.last)))
        }) else { return };
        let room = ((b - a) / 50).max(4);
        self.view(a as f64 - room as f64, b - a + 2 * room);
    }

    /// The chart's window: `span` steps from `start`, kept in the recording
    /// (as wide, at its ends).
    pub fn view(mut self, start: f64, span: usize) {
        let total = machine().view(|v| v.rec.steps.len()).max(1);
        let span = span.max(MIN_SPAN).min(total - 1);
        let a = (start.max(0.0) as usize).min(total - 1 - span);
        self.win.set(Some((a, a + span)));
    }

    /// The cursor to line `k` of its call, at the level chosen.
    pub fn go(self, k: usize) {
        if let Some(call) = self.call() {
            let frame = self.at.peek().as_ref().and_then(|m| m.frame);
            self.commit(Some(Moment { call, line: k, frame }));
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

    /// The moment at record `i` (in the chart: at its `step`), on frame `frame` there:
    /// the call it is in, its line there (the last at or before it).
    pub fn moment_at(&self, i: usize, step: Option<usize>, frame: Option<usize>, shown: Option<usize>, shown_lines: &[Line]) -> Option<Moment> {
        let (call, own, first) = machine().view(|v| (v.rec.syscall_at(i), v.line_of(i), v.rec.first_step(i)));
        let call = call?;
        // (A step: the line it is, its record's lines being its steps; a
        // write, a comparison: its own line; a block: its first line; else
        // the last before.)
        let line = |lines: &[Line]| match (step, own) {
            (Some(s), _) => lines.iter().position(|l| l.i == i).map(|p| p + s.saturating_sub(first)),
            (None, Some((r, nth))) => lines.iter().rposition(|l| l.i == r && l.nth <= nth),
            (None, None) => lines.iter().position(|l| l.i == i),
        }.or_else(|| lines.iter().rposition(|l| l.i <= i)).unwrap_or(0);
        let line = if shown == Some(call) { line(shown_lines) } else { line(&Ui::lines_of(call)) };
        Some(Moment { call, line, frame })
    }

    /// The cursor to record `i`, at the level chosen.
    pub fn go_record(self, d: Derived, i: usize) {
        let shown = d.shown.peek().as_ref().map(|m| m.call);
        let frame = self.at.peek().as_ref().and_then(|m| m.frame);
        if let Some(m) = self.moment_at(i, None, frame, shown, &d.lines.read()) {
            self.commit(Some(m));
        }
    }
}
