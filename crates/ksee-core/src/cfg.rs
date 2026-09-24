//! Control flow: how often each block and each edge of a function ran.
//!
//! Blocks of different functions interleave in the trace (a call runs the
//! callee's blocks in between), so an edge is counted from the previous
//! block of the same function on the same task.

use alloc::collections::BTreeMap;

use crate::tables::Tables;
use crate::trace::Record;

#[derive(Debug, Default, PartialEq)]
pub struct Coverage {
    /// (function, block) -> times run.
    pub blocks: BTreeMap<(u32, u32), u64>,
    /// (function, from, to) -> times taken.
    pub edges: BTreeMap<(u32, u32, u32), u64>,
}

impl Coverage {
    pub fn block(&self, func: u32, n: u32) -> u64 {
        self.blocks.get(&(func, n)).copied().unwrap_or(0)
    }

    pub fn edge(&self, func: u32, from: u32, to: u32) -> u64 {
        self.edges.get(&(func, from, to)).copied().unwrap_or(0)
    }
}

/// Count block and edge executions in `records`, for the functions `keep`
/// accepts (counting everything is fine too, but larger).
pub fn coverage(tables: &Tables, records: impl IntoIterator<Item = Record>,
                keep: impl Fn(u32) -> bool) -> Coverage {
    let mut cov = Coverage::default();
    let mut task = 0;
    // (task, function) -> last block run
    let mut last: BTreeMap<(u32, u32), u32> = BTreeMap::new();
    for r in records {
        match r {
            Record::Enter { current, .. } | Record::Exit { current, .. } => task = current,
            Record::Block { func, n } if keep(func) => {
                *cov.blocks.entry((func, n)).or_default() += 1;
                if let Some(prev) = last.insert((task, func), n) {
                    let is_edge = tables.successors.get(&(func, prev))
                        .is_some_and(|s| s.contains(&n));
                    if is_edge {
                        *cov.edges.entry((func, prev, n)).or_default() += 1;
                    }
                }
            }
            Record::Block { .. } => {}
        }
    }
    cov
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::{decode, tests::*};

    #[test]
    fn counts_edges_across_calls() {
        // f: 0 -> {1, 2}, 1 -> 2. g is called from block 1.
        let t = Tables::parse("E\t1\t0\t1,2\nE\t1\t1\t2\nE\t1\t2\t\n", "");
        let mut w = alloc::vec::Vec::new();
        for (f, n) in [(1, 0), (1, 1), (2, 0), (2, 1), (1, 2), (1, 0), (1, 2)] {
            w.extend(block(f, n));
        }
        let cov = coverage(&t, decode(&w), |f| f == 1);
        assert_eq!(cov.block(1, 0), 2);
        assert_eq!(cov.block(2, 0), 0);
        assert_eq!((cov.edge(1, 0, 1), cov.edge(1, 1, 2), cov.edge(1, 0, 2)), (1, 1, 1));
        // 2 -> 0 is not an edge (a new call of f).
        assert_eq!(cov.edge(1, 2, 0), 0);
    }
}
