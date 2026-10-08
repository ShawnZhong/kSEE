//! The page's state: the snapshot (a new one after each call), and two
//! moments: the cursor (where a click or a step left it) and the one under
//! the pointer, over the chart. The page shows the pointer's, else the
//! cursor's; derived from it: its run's lines, its stack, its frame.

use std::rc::Rc;

use dioxus::prelude::*;
use ksee_core::view::{run_to, Frame, Line, RunSummary, Timeline};

use crate::machine;

/// The fewest steps the timeline's window shows.
const MIN_SPAN: f64 = 20.0;

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

/// A moment: a run (an index in the runs: a syscall, or a task between
/// them), a line it ran (an index in its lines), and the level chosen on
/// the stack there, from the outermost (0: the chart's bottom bar; none,
/// or past the stack's top: the innermost). A level, not a call: moving
/// along, it stays. (Derived::frame is its frame: an index in the stack,
/// innermost first.)
#[derive(Clone, PartialEq, Default)]
pub struct Moment {
    pub run: usize,
    pub line: usize,
    pub level: Option<usize>,
}

/// What the state makes, computed once each time what it depends on
/// changes: the cursor's run (as the snapshot has it) and lines; the
/// timeline and its window; its line, stack, and the frame shown on it; and
/// where the cursor is, in steps (its line's: the chart's line). (The
/// pointer over the timeline moves only its cross: Ui::hover.)
#[derive(Clone, Copy)]
pub struct Derived {
    pub run: Memo<Option<RunSummary>>,
    pub lines: Memo<Shared<Vec<Line>>>,
    /// The timeline's window: its first step and how many (fractional).
    pub window: Memo<(f64, f64)>,
    pub timeline: Memo<Shared<Timeline>>,
    pub line: Memo<Option<Line>>,
    pub stack: Memo<Shared<Vec<Frame>>>,
    pub frame: Memo<usize>,
    pub cursor_step: Memo<Option<f64>>,
}

/// A moment's step: its line's (the steps are the lines stepping visits).
fn moment_step(at: &Moment, lines: &[Line]) -> Option<f64> {
    let i = lines.get(at.line)?.i;
    let k = lines[..at.line].iter().rev().take_while(|l| l.i == i).count();
    Some((machine().view(|v| v.rec.first_step(i)) + k) as f64)
}

/// Run `s`'s lines (its task's).
fn run_lines(s: &ksee_core::view::RunSummary) -> Vec<Line> {
    machine().view(|v| v.lines(s.first, s.last, s.task.addr))
}

impl Derived {
    /// The memos (a component's hooks: the App's).
    pub fn new(ui: Ui) -> Derived {
        let run = use_memo(move || {
            (ui.snapshot)();
            if !ui.up() {
                return None;
            }
            let k = (ui.cursor)()?.run;
            machine().view(|v| v.run(k))
        });
        let lines = use_memo(move || {
            (ui.snapshot)();
            Shared(Rc::new(run.cloned().map_or_else(Vec::new, |s| run_lines(&s))))
        });
        // (The timeline's: none before the kernel is up; a snapshot's, once.)
        let up = move || ui.up();
        let window = use_memo(move || {
            (ui.snapshot)();
            let all = if up() { machine().view(|v| v.rec.steps.len()) } else { 0 };
            (ui.win)().unwrap_or((0.0, all as f64))
        });
        let timeline = use_memo(move || {
            (ui.snapshot)();
            Shared(Rc::new(if up() { machine().view(|v| v.timeline()) } else { Timeline::default() }))
        });
        let line = use_memo(move || lines.read().get((ui.cursor)()?.line).cloned());
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
            (ui.cursor)().and_then(|m| m.level).filter(|&l| l < n).map_or(0, |l| n - 1 - l)
        });
        let cursor_step = use_memo(move || moment_step(&(ui.cursor)()?, &lines.read()));
        Derived { run, lines, window, timeline, line, stack, frame, cursor_step }
    }
}

/// A panel: what a lesson's link opens too.
pub use ksee_core::text::LessonTab as Tab;

#[derive(Clone, Copy)]
pub struct Ui {
    /// The published recording snapshot: what reads the recording subscribes
    /// to this identity and reads it again when it changes.
    pub snapshot: Signal<u64>,
    /// The cursor (none: no run selected), and the step under the pointer,
    /// if it is over the timeline (only its cross moves with it).
    pub cursor: Signal<Option<Moment>>,
    pub hover: Signal<Option<f64>>,
    /// The timeline's window: its first step and how many, fractional
    /// (none: the whole recording).
    pub win: Signal<Option<(f64, f64)>>,
    pub tab: Signal<Tab>,
    /// The kernel: booting (none), up, or why it could not start.
    pub kernel: Signal<Option<Result<(), String>>>,
    pub console: Signal<String>,
    /// What the source shows instead of the cursor's place, until it
    /// moves: a definition (file, line).
    pub seen: Signal<Option<(String, u32)>>,
    /// The text looked for in the functions' names (the chart's bars).
    pub find: Signal<String>,
    /// The lesson open (an index in the lessons), the program made for
    /// it (its own, or a try), and how each of its lines went.
    pub lesson: Signal<usize>,
    pub program: Signal<Vec<crate::lesson::Statement>>,
    pub outcomes: Signal<Vec<ksee_host::Outcome>>,
}

impl Ui {
    pub fn new() -> Ui {
        Ui {
            snapshot: Signal::new(0),
            cursor: Signal::new(None),
            hover: Signal::new(None),
            win: Signal::new(None),
            tab: Signal::new(Tab::Stack),
            kernel: Signal::new(None),
            console: Signal::new(String::new()),
            seen: Signal::new(None),
            find: Signal::new(String::new()),
            lesson: Signal::new(0),
            program: Signal::new(crate::lessons().first().map(|e| e.program.clone()).unwrap_or_default()),
            outcomes: Signal::new(Vec::new()),
        }
    }

    /// A program begun: what the last made cleared.
    pub fn begin_program(mut self) {
        self.outcomes.write().clear();
        self.console.set(String::new());
        self.cursor.set(None);
        self.hover.set(None);
        self.seen.set(None);
    }

    /// Lesson `k` open, with `program` (its own, or a try): made on a
    /// fresh kernel.
    pub fn open(mut self, k: usize, program: Vec<crate::lesson::Statement>) {
        self.lesson.set(k);
        self.program.set(program);
    }

    /// Lesson `k`, with its own program.
    pub fn open_lesson(self, k: usize) {
        self.open(k, crate::lessons()[k].program.clone());
    }

    pub fn commit(mut self, moment: Option<Moment>) {
        self.hover.set(None);
        self.seen.set(None);
        self.cursor.set(moment);
    }

    pub fn set_hover(mut self, step: Option<f64>) {
        if *self.hover.peek() != step {
            self.hover.set(step);
        }
    }

    /// Whether the kernel is up (booted, not failed).
    pub fn up(&self) -> bool {
        matches!((self.kernel)(), Some(Ok(())))
    }

    /// The level chosen (the cursor's; see Moment).
    pub fn level(&self) -> Option<usize> {
        self.cursor.peek().as_ref().and_then(|m| m.level)
    }

    /// The run selected (the cursor's).
    pub fn run(&self) -> Option<usize> {
        self.cursor.read().as_ref().map(|m| m.run)
    }

    /// Run `k`'s lines (its task's), as the snapshot has them.
    pub fn run_lines(k: usize) -> Vec<Line> {
        machine().view(|v| v.run(k)).map_or_else(Vec::new, |s| run_lines(&s))
    }

    /// Select run `k` (an index in the runs), the cursor at its own code's
    /// first line (a syscall's: past the syscall entry's), on its innermost
    /// frame. (The window is the reader's: it moves only to keep the cursor
    /// in it, see Timeline.)
    pub fn select(self, k: usize) {
        let own = Ui::run_lines(k).iter().position(|l| l.func != ksee_core::arch::SYSCALL);
        self.commit(Some(Moment { run: k, line: own.unwrap_or(0), level: None }));
    }

    /// The timeline's window: `span` steps from `start`, kept in the
    /// recording (as wide, at its ends).
    pub fn view(mut self, start: f64, span: f64) {
        let total = machine().view(|v| v.rec.steps.len()) as f64;
        let span = span.clamp(MIN_SPAN.min(total), total.max(1.0));
        self.win.set(Some((start.clamp(0.0, (total - span).max(0.0)), span)));
    }

    /// The cursor to line `k` of its run, at the level chosen.
    pub fn go(self, k: usize) {
        if let Some(run) = self.run() {
            self.commit(Some(Moment { run, line: k, level: self.level() }));
        }
    }

    /// The cursor at the next (`dir` 1) or previous (`dir` -1) run of
    /// `file`'s `line`, within its run.
    pub fn run_to(self, file: &str, line: u32, dir: isize) {
        let Some(at) = self.cursor.peek().clone() else { return };
        let lines = Ui::run_lines(at.run);
        if let Some(k) = run_to(&lines, at.line, file, line, dir) {
            self.go(k);
        }
    }

    /// The moment at record `i` (in the chart: at its `step`), at level `level` there:
    /// the run it is in, its line there (the last at or before it).
    pub fn moment_at(i: usize, step: Option<usize>, level: Option<usize>) -> Option<Moment> {
        let (run, own, first) = machine().view(|v| (v.rec.run_at(i), v.line_of(i), v.rec.first_step(i)));
        let run = run?;
        // (A step: the line it is, its record's lines being its steps; a
        // write, a comparison: its own line; a block: its first line; else
        // the last before.)
        let line = |lines: &[Line]| match (step, own) {
            (Some(s), _) => lines.iter().position(|l| l.i == i).map(|p| p + s.saturating_sub(first)),
            (None, Some((r, nth))) => lines.iter().rposition(|l| l.i == r && l.nth <= nth),
            (None, None) => lines.iter().position(|l| l.i == i),
        }.or_else(|| lines.iter().rposition(|l| l.i <= i)).unwrap_or(0);
        let line = line(&Ui::run_lines(run));
        Some(Moment { run, line, level })
    }

    /// The cursor to record `i`, at the level chosen.
    pub fn go_record(self, i: usize) {
        if let Some(m) = Ui::moment_at(i, None, self.level()) {
            self.commit(Some(m));
        }
    }
}
