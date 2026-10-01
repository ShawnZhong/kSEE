//! The last snapshot of the recording, for the page: views of it (see
//! ksee_core::view), at a moment's memory too, and the lookups the page
//! makes of it.

use ksee_core::view::View;
use ksee_core::Memory;

use crate::machine::Machine;
use crate::state::{k, tables, Direct};

impl Machine {
    /// Run `f` on a view of the last snapshot (ksee_core::view).
    pub fn view<T>(&self, f: impl FnOnce(&View) -> T) -> T {
        let rec = self.rec.borrow();
        f(&View { tables: tables(), rec: &rec, now: &Direct })
    }

    /// Run `f` on a view of the last snapshot and on memory as it was
    /// after record `i`: the views of a moment (its stack's values).
    pub fn at_moment<T>(&self, i: usize, f: impl FnOnce(&View, &dyn Memory) -> T) -> T {
        let rec = self.rec.borrow();
        let mem = rec.memory_at(&Direct, i);
        f(&View { tables: tables(), rec: &rec, now: &Direct }, &mem)
    }

    /// How many records the last snapshot holds.
    pub fn records(&self) -> usize {
        self.rec.borrow().len()
    }

    /// Call `id`'s syscall (see Machine::make), an index in the view's
    /// syscalls: the first of its number its runner made since it was
    /// asked, if the snapshot has it.
    pub fn syscall_of(&self, id: u32) -> Option<usize> {
        let (task, nr, from) = k(|k| k.calls.borrow().get(&id).map(|c| (c.task, c.nr, c.from)))?;
        let rec = self.rec.borrow();
        rec.syscalls.iter().position(|s| s.nr == nr && s.task == task && rec.ids[s.first] >= from)
    }

    /// The next (dir > 0) or previous record from `i` that runs line `line`
    /// of `file` (see Recording::run_of): "run to" a line.
    pub fn run_of(&self, file: &str, line: u32, i: usize, dir: i32) -> Option<usize> {
        self.rec.borrow().run_of(tables(), file, line, i, dir)
    }

    /// The first call of function `name` in records `first..=last` (of the
    /// task at address `task`, or any): its record.
    pub fn first_call(&self, name: &str, first: usize, last: usize, task: Option<u32>) -> Option<usize> {
        self.rec.borrow().first_call(tables(), name, first, last, task)
    }

    /// Syscall `name`'s parameters, each as C declares it (Types::params_of):
    /// ["int dfd", "char *filename", …]; none if this kernel has no syscall `name`.
    pub fn params(&self, name: &str) -> Option<Vec<String>> {
        tables().types.params_of(name)
    }

    /// A program written on one line: its lines (ksee_core::program::lines,
    /// `;` outside strings between them).
    pub fn program_lines(&self, text: &str) -> Vec<String> {
        ksee_core::program::lines(text).into_iter().map(String::from).collect()
    }

    /// A call's result as the page says it (Types::result): "3", "0x1d20000", "-2 (ENOENT)".
    pub fn result(&self, v: i64) -> String {
        tables().types.result(v)
    }
}
