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
    /// The Store records, oldest first.
    stores: Vec<usize>,
}

impl History {
    pub fn new(records: &[Record]) -> History {
        let mut h = History::default();
        for (i, r) in records.iter().enumerate() {
            if let Record::Store { addr, size, old, .. } = *r {
                h.stores.push(i);
                h.by_addr.entry(addr).or_default().push((i, size, old));
            }
        }
        h
    }

    /// The stores in records `from..=to`, oldest first.
    pub fn between(&self, from: usize, to: usize) -> &[usize] {
        &self.stores[self.stores.partition_point(|&i| i < from)..self.stores.partition_point(|&i| i <= to)]
    }

    /// Undo in `buf` (memory at `addr`, as the trace ended) the stores after
    /// record `at`: each byte as the oldest later store covering it found it.
    fn undo(&self, addr: u32, buf: &mut [u8], at: usize) {
        let end = addr.saturating_add(buf.len() as u32);
        // (Each byte's undoing store so far: the oldest wins.)
        let mut by = alloc::vec![usize::MAX; buf.len()];
        for (&start, list) in self.by_addr.range(addr.saturating_sub(7)..end) {
            // (Oldest first: once one covers every byte a store here can, no later one matters.)
            let most = end.min(start.saturating_add(8));
            for &(i, size, old) in &list[list.partition_point(|&(i, _, _)| i <= at)..] {
                for b in start.max(addr)..end.min(start.saturating_add(size)) {
                    let k = (b - addr) as usize;
                    if i < by[k] {
                        (by[k], buf[k]) = (i, (old >> (8 * (b - start))) as u8);
                    }
                }
                if start.saturating_add(size) >= most {
                    break;
                }
            }
        }
    }

    /// The newest store to any of `addr..addr + len` at or before record
    /// `at`: (its record, the old value).
    pub(crate) fn last_write(&self, addr: u32, len: u32, at: usize) -> Option<(usize, u64)> {
        // (Each start's newest by then that reaches `addr`; of those, the newest.)
        self.by_addr.range(addr.saturating_sub(7)..addr.saturating_add(len)).filter_map(|(&start, list)| {
            list[..list.partition_point(|&(i, _, _)| i <= at)].iter().rev()
                .find(|&&(_, size, _)| start.saturating_add(size) > addr).map(|&(i, _, old)| (i, old))
        }).max_by_key(|&(i, _)| i)
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
        self.history.undo(addr, buf, self.at);
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
    fn a_slot_is_written_by_a_store_that_covers_it() {
        let r = [Record::Store { addr: 0x10, size: 4, old: 0, line: 0, from: None }, Record::Block { func: 1, n: 1 }, Record::Store { addr: 0x20, size: 8, old: 0, line: 0, from: None }];
        let h = History::new(&r);
        assert_eq!((h.last_write(0x10, 4, 2), h.last_write(0x24, 4, 2), h.last_write(0x24, 4, 1)), (Some((0, 0)), Some((2, 0)), None));
        assert_eq!(h.last_write(0x28, 4, 2), None);
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
        assert_eq!((h.between(1, 4).len(), h.between(3, 4).len(), h.between(5, 4).len()), (2, 1, 0));
        // (Bytes of one store undone by an older, narrower one at another start.)
        let r2 = [b, Record::Store { addr: 5, size: 1, old: 9, line: 0, from: None }, Record::Store { addr: 4, size: 4, old: 0x0102_0304, line: 0, from: None }];
        let h2 = History::new(&r2);
        assert_eq!(PastMemory { base: &live, history: &h2, at: 0 }.read_u32(4), 0x0102_0904);
        assert_eq!(PastMemory { base: &live, history: &h2, at: 1 }.read_u32(4), 0x0102_0304);
        assert_eq!(h.last_write(4, 4, 2), Some((1, 1)));
        assert_eq!(h.last_write(4, 4, 0), None);
    }
}
