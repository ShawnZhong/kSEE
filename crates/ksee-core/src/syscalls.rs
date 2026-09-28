//! Syscalls in the trace: the calls of the syscall entry (wasm_syscall: its
//! call record's arguments are the number and the syscall's arguments; its
//! return's value is the result), per task.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::trace::{Args, Record};

#[derive(Debug, Clone, PartialEq)]
pub struct Syscall {
    pub task: u32,
    pub nr: u32,
    pub args: [u32; 5],
    /// The result (None: still running at the end of the trace).
    pub ret: Option<i32>,
    /// Its first and last records.
    pub first: usize,
    pub last: Option<usize>,
}

/// The syscalls in `records` (`entry`: the syscall entry's function id;
/// `tasks[i]`: the task running at record `i`), in the order they began.
pub fn split(entry: u32, records: &[Record], tasks: &[u32], args: &Args) -> Vec<Syscall> {
    let mut done = Vec::new();
    let mut open: BTreeMap<u32, Syscall> = BTreeMap::new();
    for (i, r) in records.iter().enumerate() {
        let task = tasks[i];
        match *r {
            Record::Block { func, n: 0 } if func == entry => {
                let mut a = [0; 6];
                if let Some((w, _)) = args.get(i) {
                    a.iter_mut().zip(w).for_each(|(a, &w)| *a = w);
                }
                let s = Syscall { task, nr: a[0], args: [a[1], a[2], a[3], a[4], a[5]], ret: None,
                                  first: i, last: None };
                if let Some(old) = open.insert(task, s) {
                    done.push(old); // (it never returned)
                }
            }
            Record::Return { func, value } if func == entry => {
                if let Some(mut s) = open.remove(&task) {
                    s.ret = Some(value as i32);
                    s.last = Some(i);
                    done.push(s);
                }
            }
            _ => {}
        }
    }
    done.extend(open.into_values());
    done.sort_by_key(|s| s.first);
    done
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
        let s = split(1, &r, &tasks, &args);
        assert_eq!(s.len(), 1);
        assert_eq!((s[0].task, s[0].nr, s[0].args[0], s[0].ret, s[0].first, s[0].last), (7, 64, 1, Some(3), 0, Some(3)));
    }
}
