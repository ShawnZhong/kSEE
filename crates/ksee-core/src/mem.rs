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
    /// Start address -> [(record, size, old)], oldest first.
    by_addr: BTreeMap<u32, Vec<(usize, u32, u64)>>,
    /// Ranges of unknown old contents: (address, length, record).
    unknown: Vec<(u32, u32, usize)>,
    /// The Store records, oldest first.
    stores: Vec<usize>,
}

impl History {
    pub fn new(records: &[Record]) -> History {
        let mut h = History::default();
        for (i, r) in records.iter().enumerate() {
            if let Record::Store { addr, size, old } = *r {
                h.stores.push(i);
                if size == 0 {
                    h.unknown.push((addr, old as u32, i));
                } else {
                    h.by_addr.entry(addr).or_default().push((i, size, old));
                }
            }
        }
        h
    }

    /// How many stores the trace has after record `at`.
    pub fn after(&self, at: usize) -> usize {
        self.stores.len() - self.stores.partition_point(|&i| i <= at)
    }

    /// Whether any of `addr..addr + len` is of unknown contents after `at`.
    pub fn unknown(&self, addr: u32, len: u32, at: usize) -> bool {
        self.unknown.iter().any(|&(a, l, i)| i > at && a < addr.saturating_add(len) && addr < a.saturating_add(l))
    }

    /// Byte `b` after record `at`, if a later store changed it: the old byte
    /// of the first such store.
    fn byte(&self, b: u32, at: usize) -> Option<u8> {
        let mut first: Option<(usize, u8)> = None;
        for (&start, list) in self.by_addr.range(b.saturating_sub(7)..=b) {
            // (Oldest first: the first after `at` that covers b.)
            if let Some(&(i, _, old)) = list.iter().find(|&&(i, size, _)| i > at && b - start < size)
                && first.is_none_or(|(fi, _)| i < fi) {
                    first = Some((i, (old >> (8 * (b - start))) as u8));
                }
        }
        first.map(|(_, v)| v)
    }

    /// The newest store to any of `addr..addr + len` at or before record
    /// `at`: (its record, the old value).
    pub fn last_write(&self, records: &[Record], addr: u32, len: u32, at: usize) -> Option<(usize, u64)> {
        let end = self.stores.partition_point(|&i| i <= at);
        self.stores[..end].iter().rev().find_map(|&i| match records[i] {
            Record::Store { addr: a, size, old } => {
                let n = if size == 0 { old as u32 } else { size };
                (a < addr.saturating_add(len) && addr < a.saturating_add(n)).then_some((i, old))
            }
            _ => None,
        })
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
    fn undoes_later_stores() {
        // A u32 at 4 was 1; record 1 stores 2 there, record 3 stores 3.
        let b = Record::Block { func: 1, n: 1 };
        let r = [b, Record::Store { addr: 4, size: 4, old: 1 }, b, Record::Store { addr: 4, size: 4, old: 2 }, b];
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
