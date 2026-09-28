//! Memory as it was: the stores in the trace (Store records: each with the
//! old value, see plugin/ksee-trace.cpp), undone from memory as it is (or
//! as it was when recording stopped).

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::trace::Record;
use crate::Memory;

/// The stores of a trace, indexed for reading memory as it was after any
/// of its records.
#[derive(Debug, Default)]
pub struct History {
    /// Start address -> [(record, size, old)], oldest first: the stores of
    /// 1 to 8 bytes.
    by_addr: BTreeMap<u32, Vec<(usize, u32, u64)>>,
    /// The ranges of unknown old contents (nothing to undo; rare, and
    /// long): (record, address, length), oldest first.
    ranges: Vec<(usize, u32, u32)>,
    /// The Store records, oldest first.
    stores: Vec<usize>,
}

impl History {
    pub fn new(records: &[Record]) -> History {
        let mut h = History::default();
        for (i, r) in records.iter().enumerate() {
            if let Record::Store { addr, size, old, .. } = *r {
                h.stores.push(i);
                match size {
                    0 => h.ranges.push((i, addr, old as u32)),
                    _ => h.by_addr.entry(addr).or_default().push((i, size, old)),
                }
            }
        }
        h
    }

    /// The stores in records `from..=to`, oldest first.
    pub fn between(&self, from: usize, to: usize) -> &[usize] {
        &self.stores[self.stores.partition_point(|&i| i < from)..self.stores.partition_point(|&i| i <= to)]
    }

    /// How many stores the trace has after record `at`.
    pub fn after(&self, at: usize) -> usize {
        self.stores.len() - self.stores.partition_point(|&i| i <= at)
    }

    /// Byte `b` after record `at`, if a later store changed it: the old byte
    /// of the first such store.
    fn byte(&self, b: u32, at: usize) -> Option<u8> {
        let mut first: Option<(usize, u8)> = None;
        for (&start, list) in self.by_addr.range(b.saturating_sub(7)..=b) {
            // (Oldest first: the first after `at` that covers b.)
            let later = &list[list.partition_point(|&(i, _, _)| i <= at)..];
            if let Some(&(i, _, old)) = later.iter().find(|&&(_, size, _)| b - start < size)
                && first.is_none_or(|(fi, _)| i < fi) {
                    first = Some((i, (old >> (8 * (b - start))) as u8));
                }
        }
        first.map(|(_, v)| v)
    }

    /// Whether a store in records `from..=to` wrote any of `addr..addr +
    /// len` (a range of unknown old contents aside).
    pub fn written(&self, addr: u32, len: u32, from: usize, to: usize) -> bool {
        self.by_addr.range(addr.saturating_sub(7)..addr.saturating_add(len)).any(|(&start, list)| {
            let later = &list[list.partition_point(|&(i, _, _)| i < from)..];
            later.iter().take_while(|&&(i, _, _)| i <= to).any(|&(_, size, _)| start.saturating_add(size) > addr)
        })
    }

    /// The stores to any of `addr..addr + len`, oldest first: their records.
    pub fn writes(&self, addr: u32, len: u32) -> Vec<usize> {
        let end = addr.saturating_add(len);
        let mut out: Vec<usize> = self.by_addr.range(addr.saturating_sub(7)..end)
            .flat_map(|(&start, list)| list.iter().filter(move |&&(_, size, _)| start.saturating_add(size) > addr).map(|&(i, _, _)| i))
            .chain(self.ranges.iter().filter(|&&(_, a, n)| a < end && addr < a.saturating_add(n)).map(|&(i, _, _)| i))
            .collect();
        out.sort_unstable();
        out
    }

    /// The newest store to any of `addr..addr + len` at or before record
    /// `at`: (its record, the old value: a range's, its length).
    pub fn last_write(&self, records: &[Record], addr: u32, len: u32, at: usize) -> Option<(usize, u64)> {
        let i = self.writes(addr, len).into_iter().take_while(|&i| i <= at).last()?;
        let Record::Store { old, .. } = records[i] else { return None };
        Some((i, old))
    }
}

/// Memory as it was after record `at`: `base` (memory as it is, or as it
/// was at the end of the trace) with the stores after `at` undone.
pub struct PastMemory<'a> {
    pub base: &'a dyn Memory,
    pub history: &'a History,
    pub at: usize,
}

impl Memory for PastMemory<'_> {
    fn read(&self, addr: u32, buf: &mut [u8]) {
        self.base.read(addr, buf);
        for (k, b) in buf.iter_mut().enumerate() {
            if let Some(v) = self.history.byte(addr.wrapping_add(k as u32), self.at) {
                *b = v;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Mem(Vec<u8>);
    impl Memory for Mem {
        fn read(&self, addr: u32, buf: &mut [u8]) {
            buf.copy_from_slice(&self.0[addr as usize..addr as usize + buf.len()]);
        }
    }

    #[test]
    fn a_slot_is_written_in_a_span_or_not() {
        let r = [Record::Store { addr: 0x10, size: 4, old: 0, line: 0, from: None }, Record::Block { func: 1, n: 1 }, Record::Store { addr: 0x20, size: 8, old: 0, line: 0, from: None }];
        let h = History::new(&r);
        assert!(h.written(0x10, 4, 0, 2) && !h.written(0x10, 4, 1, 2));
        assert!(h.written(0x24, 4, 1, 2) && !h.written(0x28, 4, 0, 2));
    }

    #[test]
    fn undoes_later_stores() {
        // A u32 at 4 was 1; record 1 stores 2 there, record 3 stores 3.
        let b = Record::Block { func: 1, n: 1 };
        let r = [b, Record::Store { addr: 4, size: 4, old: 1, line: 0, from: None }, b, Record::Store { addr: 4, size: 4, old: 2, line: 0, from: None }, b];
        let mut m = alloc::vec![0u8; 16];
        m[4] = 3;
        let live = Mem(m);
        let h = History::new(&r);
        let at = |i: usize| PastMemory { base: &live, history: &h, at: i }.read_u32(4);
        assert_eq!((at(0), at(2), at(4)), (1, 2, 3));
        assert_eq!((h.after(0), h.after(2), h.after(4)), (2, 1, 0));
        assert_eq!(h.last_write(&r, 4, 4, 2), Some((1, 1)));
        assert_eq!(h.last_write(&r, 4, 4, 0), None);
    }
}
