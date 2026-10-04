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
    pub(crate) fn records(&self) -> usize {
        self.rec.borrow().len()
    }

    /// Call `id`'s syscall (see Machine::make), an index in the view's
    /// syscalls: the first of its number its runner made since it was
    /// asked, if the snapshot has it.
    pub fn syscall_of(&self, id: u32) -> Option<usize> {
        let (task, nr, from) = k(|k| k.p.calls.get(&id).map(|c| (c.task, c.nr, c.from)))?;
        let rec = self.rec.borrow();
        rec.syscalls.iter().position(|s| s.nr == nr && s.task == task && rec.ids[s.first] >= from)
    }

    /// The first call of function `name` in records `first..=last` (of the
    /// task at address `task`, or any): its record.
    pub fn first_call(&self, name: &str, first: usize, last: usize, task: Option<u32>) -> Option<usize> {
        self.rec.borrow().first_call(tables(), name, first, last, task)
    }

    /// The kernel's tables: its types (a call's result as the page says it,
    /// Types::result; a syscall's parameters, Types::params_of).
    pub fn tables(&self) -> &'static ksee_core::tables::Tables {
        tables()
    }
}
