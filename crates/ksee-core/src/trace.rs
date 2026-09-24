//! Decoding the trace ring (the record format is in plugin/ksee-trace.cpp).

use alloc::vec::Vec;

use crate::Memory;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Record {
    Block { func: u32, n: u32 },
    Enter { func: u32, time: u64, current: u32, args: [u32; 4] },
    Exit { func: u32, time: u64, current: u32, value: u32 },
}

const BLOCK: u32 = 1;
const ENTER: u32 = 2;
const EXIT: u32 = 3;
const VALUE: u32 = 8;

/// Records in `words`, oldest first, starting at a pair boundary. Leading
/// continuations (of a record the ring has overwritten) and truncated
/// records are skipped.
pub fn decode(words: &[u32]) -> Vec<Record> {
    Decoder::new(words.len() as u32 / 2, |i| (words[2 * i as usize], words[2 * i as usize + 1]))
        .collect()
}

/// The ring at `trace` (`{ u32 pos; u32 ring[words]; }`), decoded in place.
pub fn ring(mem: &dyn Memory, trace: u32, words: u32) -> impl Iterator<Item = Record> + '_ {
    let (first, len) = window(mem, trace, words);
    let pairs = words / 2;
    Decoder::new(len, move |i| {
        let slot = (first + i) % pairs;
        let a = trace + 4 + slot * 8;
        (mem.read_u32(a), mem.read_u32(a + 4))
    })
}

/// The ring's contents, oldest word first.
pub fn ring_words(mem: &dyn Memory, trace: u32, words: u32) -> Vec<u32> {
    let (first, len) = window(mem, trace, words);
    let pairs = words / 2;
    let mut out = Vec::with_capacity(len as usize * 2);
    for i in 0..len {
        let a = trace + 4 + (first + i) % pairs * 8;
        out.extend([mem.read_u32(a), mem.read_u32(a + 4)]);
    }
    out
}

/// (first pair, number of pairs) of the ring's current contents.
pub fn window(mem: &dyn Memory, trace: u32, words: u32) -> (u32, u32) {
    let pos = mem.read_u32(trace) & !1;
    let len = pos.min(words) / 2;
    (pos / 2 - len, len)
}

/// Decodes `len` pairs obtained from `pair(i)`.
pub struct Decoder<F> {
    pair: F,
    next: u32,
    len: u32,
}

impl<F: FnMut(u32) -> (u32, u32)> Decoder<F> {
    pub fn new(len: u32, pair: F) -> Self {
        Decoder { pair, next: 0, len }
    }
}

impl<F: FnMut(u32) -> (u32, u32)> Iterator for Decoder<F> {
    type Item = Record;

    fn next(&mut self) -> Option<Record> {
        while self.next < self.len {
            let (func, tag) = (self.pair)(self.next);
            self.next += 1;
            let kind = tag >> 28;
            let count = match kind {
                BLOCK => return Some(Record::Block { func, n: tag & 0x0fff_ffff }),
                ENTER => 7,
                EXIT => 4,
                _ => continue, // a continuation without its head
            };
            let mut v = [0u32; 7];
            let mut got = 0;
            while got < count && self.next < self.len {
                let (value, t) = (self.pair)(self.next);
                if t >> 28 != VALUE {
                    break; // truncated: this is the next head
                }
                v[got] = value;
                got += 1;
                self.next += 1;
            }
            if got < count {
                continue;
            }
            let time = v[0] as u64 | (v[1] as u64) << 32;
            return Some(if kind == ENTER {
                Record::Enter { func, time, current: v[2], args: [v[3], v[4], v[5], v[6]] }
            } else {
                Record::Exit { func, time, current: v[2], value: v[3] }
            });
        }
        None
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn enter(func: u32, time: u64, current: u32, args: [u32; 4]) -> Vec<u32> {
        let mut w = alloc::vec![func, ENTER << 28];
        for (i, v) in [time as u32, (time >> 32) as u32, current].into_iter().chain(args).enumerate() {
            w.extend([v, VALUE << 28 | i as u32]);
        }
        w
    }

    pub fn exit(func: u32, time: u64, current: u32, value: u32) -> Vec<u32> {
        let mut w = alloc::vec![func, EXIT << 28];
        for (i, v) in [time as u32, (time >> 32) as u32, current, value].into_iter().enumerate() {
            w.extend([v, VALUE << 28 | i as u32]);
        }
        w
    }

    pub fn block(func: u32, n: u32) -> Vec<u32> {
        alloc::vec![func, BLOCK << 28 | n]
    }

    #[test]
    fn decodes_and_resyncs() {
        let mut w = enter(5, 1 << 33 | 7, 0x100, [1, 2, 3, 4]);
        let whole = w.len();
        w.extend(block(5, 3));
        w.extend(exit(5, 9, 0x100, 42));
        // Drop the head and time of the first record, as a wrapped ring would.
        let got = decode(&w[4..]);
        assert_eq!(got, [Record::Block { func: 5, n: 3 },
                         Record::Exit { func: 5, time: 9, current: 0x100, value: 42 }]);
        assert_eq!(decode(&w[..whole])[0],
                   Record::Enter { func: 5, time: 1 << 33 | 7, current: 0x100, args: [1, 2, 3, 4] });
    }

    struct Ring(Vec<u8>);

    #[test]
    fn reads_ring_words() {
        let mut m = alloc::vec![0u8; 20];
        m[0..4].copy_from_slice(&6u32.to_le_bytes());
        for (slot, w) in [4u32, 5, 2, 3].iter().enumerate() {
            m[4 + slot * 4..8 + slot * 4].copy_from_slice(&w.to_le_bytes());
        }
        assert_eq!(ring_words(&Ring(m), 0, 4), [2, 3, 4, 5]);
    }
    impl Memory for Ring {
        fn read(&self, addr: u32, buf: &mut [u8]) {
            buf.copy_from_slice(&self.0[addr as usize..addr as usize + buf.len()]);
        }
    }

    #[test]
    fn reads_a_wrapped_ring() {
        // A 4-word ring after 6 words (blocks 0, 1, 2) were written.
        let mut m = alloc::vec![0u8; 20];
        m[0..4].copy_from_slice(&6u32.to_le_bytes());
        let words = [block(9, 2), block(9, 1)].concat();
        for (slot, w) in words.iter().enumerate() {
            m[4 + slot * 4..8 + slot * 4].copy_from_slice(&w.to_le_bytes());
        }
        let got: Vec<Record> = ring(&Ring(m), 0, 4).collect();
        assert_eq!(got, [Record::Block { func: 9, n: 1 }, Record::Block { func: 9, n: 2 }]);
    }
}
