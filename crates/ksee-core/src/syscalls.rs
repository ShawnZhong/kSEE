//! What code ran for each system call: the trace split per syscall.
//!
//! Syscalls are delimited by the enter and exit records of the syscall
//! entry function (wasm_syscall). Enter and exit records carry the current
//! task; blocks in between belong to that task. The exit of context_switch
//! happens in the task switched to, so blocks follow the task that runs
//! them even when a syscall sleeps.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::trace::Record;

/// The syscall entry function of the wasm port.
pub const ENTRY: &str = "wasm_syscall";

#[derive(Debug, Clone, PartialEq)]
pub struct Syscall {
    pub task: u32,
    pub nr: u32,
    pub args: [u32; 3],
    pub start: u64,
    /// `None` if the syscall had not returned when the trace ended.
    pub end: Option<u64>,
    pub ret: Option<i32>,
    /// Blocks executed by the task during the call, in order.
    pub blocks: Vec<(u32, u32)>,
    /// Whether the ring had overwritten the start of the call.
    pub partial: bool,
    /// Positions of its enter and exit records in the record stream.
    pub first: usize,
    pub last: Option<usize>,
}

/// Split `records` at the enter and exit records of `entry` (the syscall
/// entry function's id, see [`crate::tables::Tables::find`]).
pub fn split(entry: u32, records: impl IntoIterator<Item = Record>) -> Vec<Syscall> {
    let mut done = Vec::new();
    let mut open: BTreeMap<u32, Syscall> = BTreeMap::new();
    let mut current = None;
    for (i, r) in records.into_iter().enumerate() {
        match r {
            Record::Enter { func, time, current: task, args } => {
                current = Some(task);
                if func == entry {
                    open.insert(task, Syscall {
                        task, nr: args[0], args: [args[1], args[2], args[3]], start: time,
                        end: None, ret: None, blocks: Vec::new(), partial: false,
                        first: i, last: None,
                    });
                }
            }
            Record::Exit { func, time, current: task, value } => {
                current = Some(task);
                if func == entry {
                    if let Some(mut s) = open.remove(&task) {
                        s.end = Some(time);
                        s.ret = Some(value as i32);
                        s.last = Some(i);
                        done.push(s);
                    }
                }
            }
            Record::Block { func, n } => {
                if let Some(s) = current.and_then(|t| open.get_mut(&t)) {
                    s.blocks.push((func, n));
                }
            }
            Record::Return { .. } | Record::Line { .. } | Record::Task { .. } => {}
        }
    }
    done.extend(open.into_values());
    done.sort_by_key(|s| s.start);
    done
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tables::Tables;
    use crate::trace::{decode, tests::*};

    #[test]
    fn blocks_follow_the_running_task() {
        let (sys, cs) = (Tables::id(ENTRY), Tables::id("context_switch"));
        let (a, b) = (0x100, 0x200);
        let mut w = Vec::new();
        w.extend(enter(sys, 1, a, [64, 1, 2, 3])); // a: write(1, 2, 3)
        w.extend(block(9, 0));
        w.extend(enter(cs, 2, a, [0, a, b, 0]));    // a sleeps, b runs
        w.extend(exit(cs, 3, b, 0));
        w.extend(block(9, 1));                      // b's code
        w.extend(enter(cs, 4, b, [0, b, a, 0]));
        w.extend(exit(cs, 5, a, 0));                // a resumes
        w.extend(block(9, 2));
        w.extend(exit(sys, 6, a, 36));
        let calls = split(sys, decode(&w));
        assert_eq!(calls.len(), 1);
        let c = &calls[0];
        assert_eq!((c.task, c.nr, c.args, c.ret), (a, 64, [1, 2, 3], Some(36)));
        assert_eq!(c.blocks, [(9, 0), (9, 2)]);
    }
}
