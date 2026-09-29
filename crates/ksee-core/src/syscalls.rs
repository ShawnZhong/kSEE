//! Syscalls in the trace: the calls of the syscall entry (wasm_syscall: its
//! call record's arguments are the number and the syscall's arguments; its
//! return's value is the result).

use alloc::vec::Vec;

use crate::stack::Calls;
use crate::trace::{Args, Record};

#[derive(Debug, Clone, PartialEq)]
pub struct Syscall {
    pub task: u32,
    pub nr: u32,
    /// The result (None: still running at the end of the trace, or it
    /// never returned).
    pub ret: Option<i32>,
    /// Its first and last records (its return's).
    pub first: usize,
    pub last: Option<usize>,
}

/// The syscalls among `calls` (`entry`: the syscall entry's function id;
/// `tasks[i]`: the task running at record `i`), in the order they began.
pub fn split(entry: u32, records: &[Record], tasks: &[u32], args: &Args, calls: &Calls) -> Vec<Syscall> {
    calls.calls.iter().filter(|c| c.func == entry && matches!(records[c.first], Record::Block { n: 0, .. })).map(|c| {
        let nr = args.get(c.first).and_then(|(w, _)| w.first().copied()).unwrap_or(0);
        let ret = c.last.and_then(|l| match records[l] {
            Record::Return { func, value } if func == entry => Some(value as i32),
            _ => None,
        });
        Syscall { task: tasks[c.first], nr, ret, first: c.first, last: ret.and(c.last) }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_per_task() {
        // Task 7 makes syscall 64 (write): its entry (1) calls 2; task 8 runs meanwhile.
        let r = [Record::Block { func: 1, n: 0 }, Record::Block { func: 2, n: 0 }, Record::Block { func: 3, n: 0 },
                 Record::Return { func: 1, value: 3 }];
        let tasks = [7, 7, 8, 7];
        let mut args = Args::default();
        args.push(0, &[64, 1, 2, 3, 0]);
        let calls = Calls::new(&crate::tables::Tables::default(), &r, &tasks);
        let s = split(1, &r, &tasks, &args, &calls);
        assert_eq!(s.len(), 1);
        assert_eq!((s[0].task, s[0].nr, s[0].ret, s[0].first, s[0].last), (7, 64, Some(3), 0, Some(3)));
    }
}
