//! The last snapshot of the recording, for the page: views of it (see
//! ksee_core::view), at a moment's memory too, and the lookups the page
//! makes of it.

use ksee_core::view::View;
use ksee_core::Memory;

use crate::machine::Machine;
use crate::state::{state, tables, Direct, Transfer};

/// A transfer the disk made during a call (Transfer): the records it was
/// handed over and taken as done at, and the task that handed it over, named.
#[derive(Clone, Debug)]
pub struct DiskTransfer {
    pub transfer: Transfer,
    pub asked: usize,
    pub done: usize,
    pub by: String,
}

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

    /// Call `id`'s run (see Machine::make), an index in the view's runs:
    /// the first syscall of its number its runner made since it was asked, if the snapshot has it.
    pub fn run_of_call_id(&self, id: u32) -> Option<usize> {
        let (task, nr, from) = state(|s| s.session.calls.get(&id).and_then(|c| Some((c.task, c.nr, c.begun?))))?;
        let rec = self.rec.borrow();
        rec.runs.iter().position(|s| s.nr == Some(nr) && s.task == task && rec.ids[s.first] >= from)
    }

    /// Where a lesson's link goes: function `func`'s first call in one of
    /// the program's calls, in order, the `n`th named `name` (its syscall's)
    /// if `call` says (else the first call that ran it); on the call's own
    /// task, else on any (one it woke, a kernel thread). Its run and record.
    pub fn find(&self, call: Option<(&str, usize)>, func: &str) -> Option<(usize, usize)> {
        let calls: Vec<(String, u32)> = state(|s| s.session.calls.values().map(|c| (c.said.0.clone(), c.id)).collect());
        let runs: Vec<usize> = match call {
            Some((name, n)) => calls.iter().filter(|c| c.0 == name).nth(n.checked_sub(1)?).and_then(|c| self.run_of_call_id(c.1)).into_iter().collect(),
            None => calls.iter().filter_map(|c| self.run_of_call_id(c.1)).collect(),
        };
        let at = |k: usize, own: bool| self.view(|v| v.run(k)).and_then(|s| self.first_call(func, s.first, s.last, own.then_some(s.task.addr)));
        [true, false].into_iter().find_map(|own| runs.iter().find_map(|&k| at(k, own).map(|i| (k, i))))
    }

    /// The first call of function `name` in records `first..=last` (of the
    /// task at address `task`, or any): its record.
    pub fn first_call(&self, name: &str, first: usize, last: usize, task: Option<u32>) -> Option<usize> {
        self.rec.borrow().first_call(tables(), name, first, last, task)
    }

    /// Every transfer the disk made, in order (a device's history: not one
    /// task's), each with its records and the task it was handed over for
    /// (DiskTransfer).
    pub fn disk(&self) -> Vec<DiskTransfer> {
        self.view(|v| {
            let ids = &v.rec.ids;
            // (A word of the trace, the record written at or after it, if
            // the recording has it: past its first record, and before its
            // end.)
            let record = |word: u32| Some(ids.partition_point(|&id| id < word)).filter(|&i| i < ids.len() && ids.first().is_some_and(|&f| word > f));
            // (Each a moment to go to, just after the driver's call handing
            // it over, in a run: one handed over between the calls (stores
            // only, recorded), or the ring has let go of, not.)
            state(|s| s.disk_log.iter().filter_map(|t| {
                let (asked, done) = (record(t.asked_word).filter(|&i| v.rec.run_at(i).is_some())?, record(t.done_word)?);
                let by = v.task(t.task, asked).named();
                Some(DiskTransfer { transfer: t.clone(), asked, done, by })
            }).collect())
        })
    }

    /// The kernel's tables: its types (a call's result as the page says it,
    /// Types::result; a syscall's parameters, Types::params_of).
    pub fn tables(&self) -> &'static ksee_core::tables::Tables {
        tables()
    }
}
