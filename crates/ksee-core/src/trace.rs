//! Decoding the trace ring (the record format is in plugin/ksee-trace.cpp).

use alloc::vec::Vec;

use crate::Memory;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Record {
    /// Block `n` of `func` (block 0: a call; its arguments are in Args).
    Block { func: u32, n: u32 },
    /// A return from `func`, with `value` (truncated to 32 bits).
    Return { func: u32, value: u32 },
    /// A store of `size` bytes at `addr`, which held `old` (size 0: a range
    /// of `old` bytes whose old contents are not logged), on `line`: which
    /// of its block's lines (Tables::segments, "|" apart, from 0); `from`:
    /// if it copies (a value loaded and stored as it is, a memcpy), where
    /// its bytes come from.
    Store { addr: u32, size: u32, old: u64, line: u32, from: Option<u32> },
    /// The task running (its task_struct): the runtime's, where a recording
    /// begins (no switch says so).
    Task { task: u32 },
    /// A comparison that decides a branch (or a select): `lhs` with `rhs`
    /// (truncated to 32 bits), by `pred` (an LLVM icmp predicate, from
    /// eq: see Pred), on `line` (as a Store's).
    Compare { lhs: u32, rhs: u32, pred: u8, line: u32 },
    /// Variable `var` of the function running (Tables::vars: a named one
    /// kept in a wasm local) takes `value` here, on `line` (as a Store's).
    Var { var: u32, line: u32, value: u64 },
}

/// A Compare's predicate, as LLVM numbers them from eq.
pub const PREDICATES: [(&str, bool); 10] =
    [("==", false), ("!=", false), (">", false), (">=", false), ("<", false), ("<=", false), (">", true), (">=", true), ("<", true), ("<=", true)];

impl Record {
    /// A Compare's operator, whether it is signed, and whether it held.
    pub fn compare(&self) -> Option<(&'static str, bool, bool)> {
        let Record::Compare { lhs, rhs, pred, .. } = *self else { return None };
        let (op, signed) = *PREDICATES.get(pred as usize)?;
        let (a, b) = if signed { (lhs as i32 as i64, rhs as i32 as i64) } else { (lhs as i64, rhs as i64) };
        let held = match op { "==" => a == b, "!=" => a != b, ">" => a > b, ">=" => a >= b, "<" => a < b, _ => a <= b };
        Some((op, signed, held))
    }
}

const BLOCK: u32 = 1;
/// The tag of a Task record (the runtime's).
pub const TASK: u32 = 3;
const RETURN: u32 = 4;
const COMPARE: u32 = 6;
/// The tag of a Store record's head (the plugin's, and the runtime's).
pub const STORE: u32 = 7;
/// A Store record's size class for a range of unknown old contents.
pub const UNKNOWN: u32 = 7;
/// The tag of a continuation pair.
pub const VALUE: u32 = 8;
const VAR: u32 = 9;

/// Offset of the ring's words in the trace header (`{ u32 pos; u32 pad;
/// u32 on; u32 pad; u32 ring[words]; }`, see plugin/ksee-trace.cpp).
pub const RING: u32 = 16;
/// Offset of the `on` word (probes record only while it is nonzero).
pub const ON: u32 = 8;

/// The calls' words, by their block 0's record: the arguments' (Tables::values
/// splits them), then the frame base (its locals in memory are at their
/// offsets from it: Types::locals).
#[derive(Debug, Default)]
pub struct Args {
    /// (record, where its words start), in order.
    at: Vec<(u32, u32)>,
    words: Vec<u32>,
}

impl Args {
    pub fn push(&mut self, record: usize, words: &[u32]) {
        self.at.push((record as u32, self.words.len() as u32));
        self.words.extend_from_slice(words);
    }

    /// The call at record `first`'s argument words and its frame base.
    pub fn get(&self, first: usize) -> Option<(&[u32], u32)> {
        let k = self.at.binary_search_by_key(&(first as u32), |a| a.0).ok()?;
        let end = self.at.get(k + 1).map_or(self.words.len(), |a| a.1 as usize);
        let (&frame, args) = self.words[self.at[k].1 as usize..end].split_last()?;
        Some((args, frame))
    }
}

/// The ring at `trace`, decoded, with each record's id: the number of the
/// pair it starts at, counting every pair ever written (so an id stays the
/// same while the ring goes on; indexes do not); and the calls' arguments.
pub fn ring(mem: &dyn Memory, trace: u32, words: u32) -> (Vec<Record>, Vec<u32>, Args) {
    // The ring's current contents: `len` pairs from pair `first`.
    let pos = mem.read_u32(trace) & !1;
    let len = pos.min(words) / 2;
    let first = pos / 2 - len;
    let pairs = words / 2;
    let mut d = Decoder::new(len, move |i| {
        let a = trace + RING + (first + i) % pairs * 8;
        (mem.read_u32(a), mem.read_u32(a + 4))
    });
    let (mut records, mut ids, mut args) = (Vec::new(), Vec::new(), Args::default());
    while let Some(r) = d.next() {
        if matches!(r, Record::Block { n: 0, .. }) {
            args.push(records.len(), &d.values);
        }
        records.push(r);
        ids.push(first + d.head);
    }
    (records, ids, args)
}

/// Decodes `len` pairs obtained from `pair(i)`.
pub struct Decoder<F> {
    pair: F,
    next: u32,
    len: u32,
    /// The pair the last record returned starts at.
    pub head: u32,
    /// The last record's values (a call's: its arguments' words, then its
    /// frame base).
    pub values: Vec<u32>,
}

impl<F: FnMut(u32) -> (u32, u32)> Decoder<F> {
    pub fn new(len: u32, pair: F) -> Self {
        Decoder { pair, next: 0, len, head: 0, values: Vec::new() }
    }
}

impl<F: FnMut(u32) -> (u32, u32)> Decoder<F> {
    /// The continuation pairs from the next one, into `values`: how many.
    fn read_values(&mut self) -> usize {
        self.values.clear();
        while self.next < self.len {
            let (value, t) = (self.pair)(self.next);
            if t >> 28 != VALUE {
                break; // the next head
            }
            self.values.push(value);
            self.next += 1;
        }
        self.values.len()
    }
}

impl<F: FnMut(u32) -> (u32, u32)> Iterator for Decoder<F> {
    type Item = Record;

    fn next(&mut self) -> Option<Record> {
        while self.next < self.len {
            let (func, tag) = (self.pair)(self.next);
            self.head = self.next;
            self.next += 1;
            let kind = tag >> 28;
            let got = self.read_values();
            let v = [self.values.first().copied().unwrap_or(0), self.values.get(1).copied().unwrap_or(0)];
            match kind {
                // (An entry block: the call's arguments, as many words as
                // they take, then its frame base.)
                BLOCK => return Some(Record::Block { func, n: tag & 0x0fff_ffff }),
                TASK => return Some(Record::Task { task: func }),
                RETURN => {
                    if got < 1 {
                        continue; // truncated
                    }
                    return Some(Record::Return { func, value: v[0] });
                }
                COMPARE => {
                    if got < 1 {
                        continue;
                    }
                    return Some(Record::Compare { lhs: func, rhs: v[0], pred: (tag & 0xf) as u8, line: tag >> 4 & 0xff_ffff });
                }
                STORE => {
                    // (The old value, or a range's length; then, a copy's, where from.)
                    let (class, line) = (tag & 0xf, tag >> 4 & 0xff_ffff);
                    let words = if class == 3 { 2 } else { 1 };
                    if got < words {
                        continue;
                    }
                    let from = self.values.get(words).copied();
                    return Some(if class == UNKNOWN {
                        Record::Store { addr: func, size: 0, old: v[0] as u64, line, from }
                    } else {
                        Record::Store { addr: func, size: 1 << class, old: v[0] as u64 | if class == 3 { (v[1] as u64) << 32 } else { 0 }, line, from }
                    });
                }
                // (A wide one's high word after it.)
                VAR => return Some(Record::Var { var: tag & 0xffff, line: tag >> 16 & 0xfff, value: func as u64 | (v[0] as u64) << 32 }),
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
    decode_values(words).into_iter().map(|(r, _)| r).collect()
}

/// The same, with each record's values.
fn decode_values(words: &[u32]) -> Vec<(Record, Vec<u32>)> {
    let mut d = Decoder::new(words.len() as u32 / 2, |i| (words[2 * i as usize], words[2 * i as usize + 1]));
    core::iter::from_fn(|| d.next().map(|r| (r, d.values.clone()))).collect()
}

    #[test]
    fn a_variable_takes_a_value() {
        // (A wide one: its high word after it.)
        let w = [5, VAR << 28 | 2, 9, VAR << 28 | 4 << 16 | 3, 1, VALUE << 28];
        assert_eq!(decode(&w), [Record::Var { var: 2, line: 0, value: 5 }, Record::Var { var: 3, line: 4, value: 9 | 1 << 32 }]);
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
        assert_eq!(decode_values(&w)[0], (Record::Block { func: 5, n: 0 }, alloc::vec![1, 2, 0x900]));
    }

    struct Ring(Vec<u8>);
    impl Memory for Ring {
        fn read(&self, addr: u32, buf: &mut [u8]) {
            buf.copy_from_slice(&self.0[addr as usize..addr as usize + buf.len()]);
        }
    }

    #[test]
    fn calls_carry_arguments_and_stores_old_values() {
        // (The store of 4 at 0x100 is on its block's line 3.)
        let w = [9, BLOCK << 28, 7, VALUE << 28, 8, VALUE << 28 | 1, 0x80, VALUE << 28 | 2, 0x100, STORE << 28 | 3 << 4 | 2, 5, VALUE << 28,
                 0x200, STORE << 28 | UNKNOWN, 64, VALUE << 28, 9, BLOCK << 28 | 1];
        assert_eq!(decode_values(&w)[0].1, [7, 8, 0x80]);
        assert_eq!(decode(&w), [Record::Block { func: 9, n: 0 },
                                Record::Store { addr: 0x100, size: 4, old: 5, line: 3, from: None }, Record::Store { addr: 0x200, size: 0, old: 64, line: 0, from: None },
                                Record::Block { func: 9, n: 1 }]);
    }

    #[test]
    fn copies_and_comparisons() {
        // A store of 4 bytes at 0x100 (it held 5) copied from 0x300; then 18 >= 0 (sge, 7), on line 2.
        let w = [0x100, STORE << 28 | 2, 5, VALUE << 28, 0x300, VALUE << 28 | 1, 18, COMPARE << 28 | 2 << 4 | 7, 0, VALUE << 28];
        let got = decode(&w);
        assert_eq!(got, [Record::Store { addr: 0x100, size: 4, old: 5, line: 0, from: Some(0x300) }, Record::Compare { lhs: 18, rhs: 0, pred: 7, line: 2 }]);
        assert_eq!(got[1].compare(), Some((">=", true, true)));
        assert_eq!(Record::Compare { lhs: -1i32 as u32, rhs: 0, pred: 3, line: 0 }.compare(), Some((">=", false, true)));
        assert_eq!(Record::Compare { lhs: 5, rhs: 5, pred: 1, line: 0 }.compare(), Some(("!=", false, false)));
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
        let (got, ids, _) = ring(&Ring(m), 0, 4);
        assert_eq!(got, [Record::Block { func: 9, n: 1 }, Record::Block { func: 9, n: 2 }]);
        assert_eq!(ids, [1, 2]);
    }
}
