//! The page's state: the snapshot (a new one after each call), the call
//! selected, the cursor in it (a line it ran: an index in its lines), and
//! the frame chosen on the Stack (none: the innermost). Derived from them:
//! the call's lines, and the moment the cursor is at.

use std::rc::Rc;

use dioxus::prelude::*;
use ksee_core::view::{Chart, Frame, Line, SyscallSummary};

use crate::machine;

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

/// What the state makes, computed once each time what it depends on
/// changes: the call selected (as the snapshot has it), its lines and its
/// chart; the stack at the cursor (and the cursor's line).
#[derive(Clone, Copy)]
pub struct Derived {
    pub syscall: Memo<Option<SyscallSummary>>,
    pub lines: Memo<Shared<Vec<Line>>>,
    pub chart: Memo<Shared<Chart>>,
    pub stack: Memo<Shared<(Vec<Frame>, Option<Line>)>>,
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
            let m = machine();
            Shared(Rc::new(syscall.cloned().map_or_else(Vec::new, |s| m.view(|v| v.lines(s.first, s.last.unwrap_or(m.records().saturating_sub(1)), s.task.addr)))))
        });
        let chart = use_memo(move || {
            let m = machine();
            Shared(Rc::new(syscall.cloned().map_or_else(Chart::default, |s| m.view(|v| v.chart(s.first, s.last.unwrap_or(m.records().saturating_sub(1)), crate::chart::COLUMNS)))))
        });
        let stack = use_memo(move || {
            let l = lines.read().get((ui.cursor)()).cloned();
            let frames = l.as_ref().map_or_else(Vec::new, |l| machine().at_moment(l.i, |v, mem| v.call_stack(l.i, Some(l.nth as usize), mem, l.i)));
            Shared(Rc::new((frames, l)))
        });
        Derived { syscall, lines, chart, stack }
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
    pub tab: Signal<Tab>,
    pub console: Signal<String>,
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
            tab: Signal::new(Tab::Stack),
            console: Signal::new(String::new()),
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

    /// Select call `k` (an index in the syscalls), the cursor at its own
    /// code's first line (past the syscall entry's), on its innermost frame.
    pub fn select(mut self, k: usize) {
        self.call.set(Some(k));
        // (Its lines, now: the memo's are the call's before until it runs again.)
        let m = machine();
        let own = m.view(|v| {
            let s = v.syscalls().into_iter().nth(k)?;
            v.lines(s.first, s.last.unwrap_or(m.records().saturating_sub(1)), s.task.addr).iter().position(|l| l.func != ksee_core::arch::SYSCALL)
        });
        self.go(own.unwrap_or(0));
    }

    /// The cursor to line `k`, on its innermost frame.
    pub fn go(mut self, k: usize) {
        self.cursor.set(k);
        self.frame.set(None);
    }
}
