//! Decoding the trace ring (the record format is in plugin/ksee-trace.cpp).

use alloc::vec::Vec;

use crate::Memory;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Record {
    /// Block `n` of `func` (block 0: a call; its arguments follow).
    Block { func: u32, n: u32 },
    /// A return from `func`, with `value` (truncated to 32 bits).
    Return { func: u32, value: u32 },
    /// The arguments of the call whose entry block (Block n 0) is just
    /// before: the first `n` of `args`; and its frame base (its locals in
    /// memory are at their offsets from it: Types::locals).
    Args { args: [u32; 6], n: u8, frame: u32 },
    /// A store of `size` bytes at `addr`, which held `old` (size 0: a range
    /// of `old` bytes whose old contents are not logged).
    Store { addr: u32, size: u32, old: u64 },
    /// The task running (its task_struct): the runtime's, where a recording
    /// begins (no switch says so).
    Task { task: u32 },
}

const BLOCK: u32 = 1;
/// The tag of a Task record (the runtime's).
pub const TASK: u32 = 3;
const RETURN: u32 = 4;
/// The tag of a Store record's head (the plugin's, and the runtime's).
pub const STORE: u32 = 7;
/// A Store record's size class for a range of unknown old contents.
pub const UNKNOWN: u32 = 7;
/// The tag of a continuation pair.
pub const VALUE: u32 = 8;

/// Offset of the ring's words in the trace header (`{ u32 pos; u32 pad;
/// u32 on; u32 pad; u32 ring[words]; }`, see plugin/ksee-trace.cpp).
pub const RING: u32 = 16;
/// Offset of the `on` word (probes record only while it is nonzero).
pub const ON: u32 = 8;

/// The ring at `trace`, decoded, with each record's id: the number of the
/// pair it starts at, counting every pair ever written (so an id stays the
/// same while the ring goes on; indexes do not).
pub fn ring(mem: &dyn Memory, trace: u32, words: u32) -> (Vec<Record>, Vec<u32>) {
    // The ring's current contents: `len` pairs from pair `first`.
    let pos = mem.read_u32(trace) & !1;
    let len = pos.min(words) / 2;
    let first = pos / 2 - len;
    let pairs = words / 2;
    let mut d = Decoder::new(len, move |i| {
        let a = trace + RING + (first + i) % pairs * 8;
        (mem.read_u32(a), mem.read_u32(a + 4))
    });
    let (mut records, mut ids) = (Vec::new(), Vec::new());
    while let Some(r) = d.next() {
        records.push(r);
        ids.push(first + d.head);
    }
    (records, ids)
}

/// Decodes `len` pairs obtained from `pair(i)`.
pub struct Decoder<F> {
    pair: F,
    next: u32,
    len: u32,
    /// The pair the last record returned starts at.
    pub head: u32,
    /// A record to return next (a call's arguments, after its block).
    pending: Option<Record>,
}

impl<F: FnMut(u32) -> (u32, u32)> Decoder<F> {
    pub fn new(len: u32, pair: F) -> Self {
        Decoder { pair, next: 0, len, head: 0, pending: None }
    }
}

impl<F: FnMut(u32) -> (u32, u32)> Decoder<F> {
    /// Up to `max` continuation pairs from the next one, into `v`: how many.
    fn values(&mut self, v: &mut [u32], max: usize) -> usize {
        let mut got = 0;
        while got < max && self.next < self.len {
            let (value, t) = (self.pair)(self.next);
            if t >> 28 != VALUE {
                break; // the next head
            }
            v[got] = value;
            got += 1;
            self.next += 1;
        }
        got
    }
}

impl<F: FnMut(u32) -> (u32, u32)> Iterator for Decoder<F> {
    type Item = Record;

    fn next(&mut self) -> Option<Record> {
        if let Some(r) = self.pending.take() {
            return Some(r);
        }
        while self.next < self.len {
            let (func, tag) = (self.pair)(self.next);
            self.head = self.next;
            self.next += 1;
            let kind = tag >> 28;
            let mut v = [0u32; 2];
            match kind {
                BLOCK => {
                    let n = tag & 0x0fff_ffff;
                    // An entry block: the call's arguments follow (as many as
                    // it has), then its frame base.
                    if n == 0 {
                        let mut v = [0u32; 7];
                        let k = self.values(&mut v, 7);
                        if k > 0 {
                            let mut args = [0u32; 6];
                            args[..k - 1].copy_from_slice(&v[..k - 1]);
                            self.pending = Some(Record::Args { args, n: (k - 1) as u8, frame: v[k - 1] });
                        }
                    }
                    return Some(Record::Block { func, n });
                }
                TASK => return Some(Record::Task { task: func }),
                RETURN => {
                    if self.values(&mut v, 1) < 1 {
                        continue; // truncated
                    }
                    return Some(Record::Return { func, value: v[0] });
                }
                STORE => {
                    let class = tag & 0xf;
                    let want = if class == 3 { 2 } else { 1 };
                    if self.values(&mut v, want) < want {
                        continue;
                    }
                    return Some(if class == UNKNOWN {
                        Record::Store { addr: func, size: 0, old: v[0] as u64 }
                    } else {
                        Record::Store { addr: func, size: 1 << class, old: v[0] as u64 | (v[1] as u64) << 32 }
                    });
                }
                _ => continue, // a continuation without its head
            }
        }
        None
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

/// Records in `words`, oldest first, starting at a pair boundary. Leading
/// continuations (of a record the ring has overwritten) and truncated
/// records are skipped.
fn decode(words: &[u32]) -> Vec<Record> {
    Decoder::new(words.len() as u32 / 2, |i| (words[2 * i as usize], words[2 * i as usize + 1]))
        .collect()
}

    pub fn block(func: u32, n: u32) -> Vec<u32> {
        alloc::vec![func, BLOCK << 28 | n]
    }

    pub fn ret(func: u32, value: u32) -> Vec<u32> {
        alloc::vec![func, RETURN << 28, value, VALUE << 28]
    }

    #[test]
    fn decodes_and_resyncs() {
        // (A call: block 0, arguments 1 and 2, the frame base 0x900.)
        let mut w = alloc::vec![5, BLOCK << 28, 1, VALUE << 28, 2, VALUE << 28 | 1, 0x900, VALUE << 28 | 2];
        w.extend(block(5, 3));
        w.extend(ret(5, 42));
        // Drop the head of the first record, as a wrapped ring would.
        assert_eq!(decode(&w[2..]), [Record::Block { func: 5, n: 3 }, Record::Return { func: 5, value: 42 }]);
        assert_eq!(decode(&w)[..2], [Record::Block { func: 5, n: 0 }, Record::Args { args: [1, 2, 0, 0, 0, 0], n: 2, frame: 0x900 }]);
    }

    struct Ring(Vec<u8>);
    impl Memory for Ring {
        fn read(&self, addr: u32, buf: &mut [u8]) {
            buf.copy_from_slice(&self.0[addr as usize..addr as usize + buf.len()]);
        }
    }

    #[test]
    fn calls_carry_arguments_and_stores_old_values() {
        let w = [9, BLOCK << 28, 7, VALUE << 28, 8, VALUE << 28 | 1, 0x80, VALUE << 28 | 2, 0x100, STORE << 28 | 2, 5, VALUE << 28,
                 0x200, STORE << 28 | UNKNOWN, 64, VALUE << 28, 9, BLOCK << 28 | 1];
        assert_eq!(decode(&w), [Record::Block { func: 9, n: 0 }, Record::Args { args: [7, 8, 0, 0, 0, 0], n: 2, frame: 0x80 },
                                Record::Store { addr: 0x100, size: 4, old: 5 }, Record::Store { addr: 0x200, size: 0, old: 64 },
                                Record::Block { func: 9, n: 1 }]);
    }

    #[test]
    fn reads_a_wrapped_ring() {
        // A 4-word ring after 6 words (blocks 0, 1, 2) were written.
        let mut m = alloc::vec![0u8; 32];
        m[0..4].copy_from_slice(&6u32.to_le_bytes());
        let words = [block(9, 2), block(9, 1)].concat();
        for (slot, w) in words.iter().enumerate() {
            let at = RING as usize + slot * 4;
            m[at..at + 4].copy_from_slice(&w.to_le_bytes());
        }
        let (got, ids) = ring(&Ring(m), 0, 4);
        assert_eq!(got, [Record::Block { func: 9, n: 1 }, Record::Block { func: 9, n: 2 }]);
        assert_eq!(ids, [1, 2]);
    }
}
