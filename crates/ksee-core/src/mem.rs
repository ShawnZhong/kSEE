//! Memory as it was: the stores the kernel made (the plugin's memory ring,
//! see plugin/ksee-trace.cpp), undone from memory as it is.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::Memory;

/// Offset of the memory ring's words (`{ u32 pos; u32 pad; u32 ring[]; }`).
pub const RING: u32 = 8;
/// `main` is the main ring's pair count modulo this.
pub const MAIN: u32 = 1 << 29;
/// A range whose old contents were not logged.
pub const UNKNOWN: u32 = 7;

/// A store: `size` bytes at `addr` held `old` (a range of unknown contents
/// if `size` is 0: `len` bytes); made when the main ring had `main` pairs
/// (modulo MAIN).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Store {
    pub addr: u32,
    pub size: u32,
    pub old: u64,
    pub main: u32,
}

/// The stores in the ring at `mem_ring` (`words` long), oldest first.
pub fn stores(mem: &dyn Memory, ring: u32, words: u32) -> Vec<Store> {
    let pos = mem.read_u32(ring) & !3;
    let n = pos.min(words) / 4;
    (0..n).map(|k| {
        let at = ring + RING + ((pos - 4 * (n - k)) & (words - 1)) * 4;
        let (addr, tag) = (mem.read_u32(at), mem.read_u32(at + 4));
        let old = mem.read_u32(at + 8) as u64 | (mem.read_u32(at + 12) as u64) << 32;
        let class = tag >> 29;
        let size = if class == UNKNOWN { 0 } else { 1 << class };
        Store { addr, size, old: if class == UNKNOWN { old & 0xffff_ffff } else { old }, main: tag & (MAIN - 1) }
    }).collect()
}

/// How far pair count `x` is behind `now` (both modulo MAIN).
fn behind(now: u32, x: u32) -> u32 {
    now.wrapping_sub(x) & (MAIN - 1)
}

/// The stores, indexed for reading memory as it was at any moment.
#[derive(Debug, Default)]
pub struct History {
    /// Start address -> [(age, size, old)], oldest first; age: how many
    /// main ring pairs ago (from `now`) it was made.
    by_addr: BTreeMap<u32, Vec<(u32, u32, u64)>>,
    /// Ranges of unknown old contents: (address, length, age).
    unknown: Vec<(u32, u32, u32)>,
    /// Every store's age, oldest first (so, decreasing).
    ages: Vec<u32>,
    now: u32,
}

impl History {
    /// `stores` oldest first; `now`: the main ring's pair count now.
    pub fn new(stores: &[Store], now: u32) -> History {
        let mut h = History { now, ..Default::default() };
        for s in stores {
            let age = behind(now, s.main);
            h.ages.push(age);
            if s.size == 0 {
                h.unknown.push((s.addr, s.old as u32, age));
            } else {
                h.by_addr.entry(s.addr).or_default().push((age, s.size, s.old));
            }
        }
        h
    }

    /// The age of the moment the main ring had `at` pairs.
    pub fn age(&self, at: u32) -> u32 {
        behind(self.now, at & (MAIN - 1))
    }

    /// How many stores were made after the moment of age `age`.
    pub fn after(&self, age: u32) -> usize {
        self.ages.len() - self.ages.partition_point(|&a| a >= age)
    }

    /// Whether the log reaches back to the moment of age `age` (a store
    /// older than it is logged, so none since is missing).
    pub fn reaches(&self, age: u32) -> bool {
        self.ages.first().is_some_and(|&a| a >= age)
    }

    /// Whether any of `addr..addr + len` is of unknown contents then.
    pub fn unknown(&self, addr: u32, len: u32, age: u32) -> bool {
        self.unknown.iter().any(|&(a, l, t)| t < age && a < addr.saturating_add(len) && addr < a.saturating_add(l))
    }

    /// Byte `b` at the moment of age `age`, if a store changed it since:
    /// the old byte of the first store after it.
    fn byte(&self, b: u32, age: u32) -> Option<u8> {
        let mut first: Option<(u32, u8)> = None; // (age, byte): the oldest after
        for (&start, list) in self.by_addr.range(b.saturating_sub(7)..=b) {
            for &(t, size, old) in list {
                if t < age && b - start < size {
                    if first.is_none_or(|(ft, _)| t > ft) {
                        first = Some((t, (old >> (8 * (b - start))) as u8));
                    }
                    break; // (oldest first: the first after is it)
                }
            }
        }
        first.map(|(_, v)| v)
    }
}

/// Memory `live` as it was at the moment of age `age` (History::age).
pub struct PastMemory<'a> {
    pub live: &'a dyn Memory,
    pub history: &'a History,
    pub age: u32,
}

impl Memory for PastMemory<'_> {
    fn read(&self, addr: u32, buf: &mut [u8]) {
        self.live.read(addr, buf);
        for (k, b) in buf.iter_mut().enumerate() {
            if let Some(v) = self.history.byte(addr.wrapping_add(k as u32), self.age) {
                *b = v;
            }
        }
    }
}

/// The newest store to any of `addr..addr + len` made before the main ring
/// had `before` pairs (`now`: as in Past::new), if the ring still has one.
pub fn last_write(stores: &[Store], addr: u32, len: u32, before: u32, now: u32) -> Option<Store> {
    let cut = behind(now, before & (MAIN - 1));
    stores.iter().rev().find(|s| {
        let size = if s.size == 0 { s.old as u32 } else { s.size };
        behind(now, s.main) >= cut && s.addr < addr.saturating_add(len) && addr < s.addr.saturating_add(size)
    }).copied()
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
        // A u32 at 4 was 1, then 2 (at pair 10), now 3 (at pair 20).
        let s = [Store { addr: 4, size: 4, old: 1, main: 10 }, Store { addr: 4, size: 4, old: 2, main: 20 }];
        let mut m = alloc::vec![0u8; 16];
        m[4] = 3;
        let live = Mem(m);
        let h = History::new(&s, 30);
        let at = |t: u32| PastMemory { live: &live, history: &h, age: h.age(t) }.read_u32(4);
        assert_eq!((at(5), at(15), at(25)), (1, 2, 3));
        assert_eq!((h.after(h.age(15)), h.reaches(h.age(15)), h.reaches(h.age(5))), (1, true, false));
        assert_eq!(last_write(&s, 4, 4, 15, 30).map(|s| s.main), Some(10));
        assert_eq!(last_write(&s, 4, 4, 5, 30), None);
    }
}
