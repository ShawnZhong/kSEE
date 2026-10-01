//! The page's state: the snapshot (a new one after each call), and two
//! moments: the cursor (where a click or a step left it) and the one under
//! the pointer, over the chart. The page shows the pointer's, else the
//! cursor's; derived from it: its call's lines, its stack, its frame.

use std::rc::Rc;

use dioxus::prelude::*;
use ksee_core::view::{Chart, Frame, Line, SyscallSummary};

use crate::machine;

/// The chart's columns: moments across its window.
const COLUMNS: usize = 400;

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
/// the record the moment is at, its stack, and the frame shown on it; and
/// the record the cursor is at (the chart's line).
#[derive(Clone, Copy)]
pub struct Derived {
    pub shown: Memo<Option<Moment>>,
    pub syscall: Memo<Option<SyscallSummary>>,
    pub lines: Memo<Shared<Vec<Line>>>,
    pub window: Memo<(usize, usize)>,
    pub chart: Memo<Shared<Chart>>,
    pub moment: Memo<Option<(usize, Option<usize>)>>,
    pub stack: Memo<Shared<Vec<Frame>>>,
    pub frame: Memo<usize>,
    pub cursor: Memo<Option<usize>>,
}

impl Derived {
    /// The memos (a component's hooks: the App's).
    pub fn new(ui: Ui) -> Derived {
        let shown = use_memo(move || (ui.peek)().or((ui.at)()));
        let syscall = use_memo(move || {
            (ui.version)();
            let k = shown()?.call;
            machine().view(|v| v.syscalls().into_iter().nth(k))
        });
        let lines = use_memo(move || {
            Shared(Rc::new(syscall.cloned().map_or_else(Vec::new, |s| {
                let m = machine();
                m.view(|v| v.lines(s.first, s.last.unwrap_or(m.records().saturating_sub(1)), s.task.addr))
            })))
        });
        // (The chart's: none before the kernel is up.)
        let up = move || matches!((ui.kernel)(), Some(Ok(())));
        let window = use_memo(move || {
            (ui.version)();
            let end = if up() { machine().records().saturating_sub(1) } else { 0 };
            (ui.win)().unwrap_or((0, end))
        });
        let chart = use_memo(move || {
            let (a, b) = window();
            Shared(Rc::new(if up() { machine().view(|v| v.chart(a, b, COLUMNS)) } else { Chart::default() }))
        });
        let moment = use_memo(move || lines.read().get(shown()?.line).map(|l| (l.i, Some(l.nth as usize))));
        let stack = use_memo(move || {
            Shared(Rc::new(moment().map_or_else(Vec::new, |(i, nth)| machine().at_moment(i, |v, mem| v.call_stack(i, nth, mem, i)))))
        });
        let frame = use_memo(move || shown().and_then(|m| m.frame).and_then(|(first, func)| stack.read().iter().position(|f| f.first == first && f.func == func)).unwrap_or(0));
        let cursor = use_memo(move || {
            let at = (ui.at)()?;
            if shown().is_some_and(|s| s.call == at.call) {
                lines.read().get(at.line).map(|l| l.i)
            } else {
                Ui::lines_of(at.call).get(at.line).map(|l| l.i)
            }
        });
        Derived { shown, syscall, lines, window, chart, moment, stack, frame, cursor }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Tab {
    Stack,
    Objects,
    Tasks,
    Console,
}

#[derive(Clone, Copy)]
pub struct Ui {
    /// Bumped at each snapshot: what reads the recording reads it again.
    pub version: Signal<u32>,
    /// The cursor (none: no call selected), and the moment under the
    /// pointer, if it is over the chart.
    pub at: Signal<Option<Moment>>,
    pub peek: Signal<Option<Moment>>,
    /// The chart's window (records; none: the whole recording).
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
    pub program: Signal<Vec<String>>,
    pub outcomes: Signal<Vec<crate::lesson::Outcome>>,
}

impl Ui {
    pub fn new() -> Ui {
        Ui {
            version: Signal::new(0),
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

    /// The call selected (the cursor's).
    pub fn call(&self) -> Option<usize> {
        self.at.read().as_ref().map(|m| m.call)
    }

    /// The call shown, as the snapshot has it (Derived's).
    pub fn syscall(&self) -> Option<SyscallSummary> {
        consume_context::<Derived>().syscall.cloned()
    }

    /// The lines the call shown ran (its task's), in order.
    pub fn lines(&self) -> Shared<Vec<Line>> {
        consume_context::<Derived>().lines.cloned()
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
    /// frame; the chart's window the whole recording, unless it has the call.
    pub fn select(mut self, k: usize) {
        let s = machine().view(|v| v.syscalls().into_iter().nth(k));
        if let (Some((a, b)), Some(s)) = ((self.win)(), s) && (s.first < a || s.last.is_none_or(|l| l > b)) {
            self.win.set(None);
        }
        let own = Ui::lines_of(k).iter().position(|l| l.func != ksee_core::arch::SYSCALL);
        self.peek.set(None);
        self.at.set(Some(Moment { call: k, line: own.unwrap_or(0), frame: None }));
    }

    /// The cursor to line `k` of its call, on its innermost frame.
    pub fn go(mut self, k: usize) {
        if let Some(call) = self.call() {
            self.peek.set(None);
            self.at.set(Some(Moment { call, line: k, frame: None }));
        }
    }

    /// The moment at record `i` (in the chart), on frame `frame` there:
    /// the call it is in, its line there (the last at or before it).
    pub fn moment_at(&self, i: usize, frame: Option<(usize, String)>) -> Option<Moment> {
        let call = machine().view(|v| v.rec.syscall_at(i))?;
        let shown = consume_context::<Derived>().shown.peek().as_ref().map(|m| m.call);
        let line = |lines: &[Line]| lines.iter().rposition(|l| l.i <= i).unwrap_or(0);
        let line = if shown == Some(call) { line(&self.lines()) } else { line(&Ui::lines_of(call)) };
        Some(Moment { call, line, frame })
    }

    /// The cursor to record `i`, on frame `frame` (see `moment_at`).
    pub fn go_record(mut self, i: usize, frame: Option<(usize, String)>) {
        if let Some(m) = self.moment_at(i, frame) {
            self.peek.set(None);
            self.at.set(Some(m));
        }
    }
}
