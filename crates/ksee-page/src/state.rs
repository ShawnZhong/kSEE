//! The page's state: the snapshot (a new one after each call), the call
//! selected, the cursor in it (a line it ran: an index in its lines), and
//! the frame chosen on the Stack (none: the innermost). Derived from them:
//! the call's lines, and the moment the cursor is at.

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

/// The moment under the pointer, over the chart: a column's record, and
/// the bar there, if one (its call's first record; code inlined into it,
/// or not).
#[derive(Clone, Copy, PartialEq)]
pub struct Peek {
    pub i: usize,
    pub call: Option<(usize, bool)>,
}

/// What the state makes, computed once each time what it depends on
/// changes: the call selected (as the snapshot has it), its lines; the
/// chart's window and the chart; the moment shown (the pointer's, else the
/// cursor's), its stack, and the frame shown on it.
#[derive(Clone, Copy)]
pub struct Derived {
    pub syscall: Memo<Option<SyscallSummary>>,
    pub lines: Memo<Shared<Vec<Line>>>,
    pub window: Memo<(usize, usize)>,
    pub chart: Memo<Shared<Chart>>,
    pub moment: Memo<Option<(usize, Option<usize>)>>,
    pub stack: Memo<Shared<Vec<Frame>>>,
    pub shown: Memo<usize>,
}

impl Derived {
    /// The memos (a component's hooks: the App's).
    pub fn new(ui: Ui) -> Derived {
        let syscall = use_memo(move || {
            (ui.version)();
            let k = (ui.call)()?;
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
        let moment = use_memo(move || match (ui.peek)() {
            Some(p) => Some((p.i, None)),
            None => lines.read().get((ui.cursor)()).map(|l| (l.i, Some(l.nth as usize))),
        });
        let stack = use_memo(move || {
            Shared(Rc::new(moment().map_or_else(Vec::new, |(i, nth)| machine().at_moment(i, |v, mem| v.call_stack(i, nth, mem, i)))))
        });
        let shown = use_memo(move || match (ui.peek)() {
            Some(p) => p.call.and_then(|c| stack.read().iter().position(|f| (f.first, f.inlined) == c)).unwrap_or(0),
            None => (ui.frame)().unwrap_or(0),
        });
        Derived { syscall, lines, window, chart, moment, stack, shown }
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
    pub call: Signal<Option<usize>>,
    pub cursor: Signal<usize>,
    pub frame: Signal<Option<usize>>,
    /// The chart's window (records; none: the whole recording), and the
    /// moment under the pointer, if it is over the chart.
    pub win: Signal<Option<(usize, usize)>>,
    pub peek: Signal<Option<Peek>>,
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
            call: Signal::new(None),
            cursor: Signal::new(0),
            frame: Signal::new(None),
            win: Signal::new(None),
            peek: Signal::new(None),
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

    /// The call selected, as the snapshot has it (Derived's).
    pub fn syscall(&self) -> Option<SyscallSummary> {
        consume_context::<Derived>().syscall.cloned()
    }

    /// The lines the call selected ran (its task's), in order.
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
        self.call.set(Some(k));
        let s = machine().view(|v| v.syscalls().into_iter().nth(k));
        if let (Some((a, b)), Some(s)) = ((self.win)(), s) && (s.first < a || s.last.is_none_or(|l| l > b)) {
            self.win.set(None);
        }
        // (Its lines, now: the memo's are the call's before until it runs again.)
        let own = Ui::lines_of(k).iter().position(|l| l.func != ksee_core::arch::SYSCALL);
        self.go(own.unwrap_or(0));
    }

    /// The cursor to line `k`, on its innermost frame.
    pub fn go(mut self, k: usize) {
        self.cursor.set(k);
        self.frame.set(None);
    }

    /// The cursor to record `i` (a moment in the chart), on frame `frame`
    /// there (from the innermost): the call it is in selected first, if
    /// another's (the window stays); its line there, the last at or before it.
    pub fn go_record(mut self, i: usize, frame: Option<usize>) {
        let m = machine();
        let k = m.view(|v| v.rec.syscall_at(i));
        let lines = match k {
            Some(k) if Some(k) != (self.call)() => {
                self.call.set(Some(k));
                Ui::lines_of(k)
            }
            _ => self.lines().0.as_ref().clone(),
        };
        self.cursor.set(lines.iter().rposition(|l| l.i <= i).unwrap_or(0));
        self.frame.set(frame);
    }
}
