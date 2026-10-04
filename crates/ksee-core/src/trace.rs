//! Decoding the trace ring (the record format is in plugin/ksee-trace.cpp).

pub mod wire;

use alloc::vec::Vec;
use core::num::NonZeroU32;

use crate::Memory;
use wire::{Kind, StoreSize};

pub use wire::RING;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Record {
    /// Block `n` of `func` (block 0: a call; its arguments are in Args).
    Block { func: u32, n: u32 },
    /// A return from `func`, with `value` (a 32-bit one sign-extended).
    Return { func: u32, value: i64 },
    /// A store of `size` bytes at `addr`, which held `old`, on `line`:
    /// which of its block's lines (tables::Block::steps); `from`:
    /// if it copies (a value loaded and stored as it is, a memcpy), where
    /// its bytes come from. (NonZero: the record says 0 for none; and a
    /// Record fits 32 bytes.)
    Store { addr: u32, size: u32, old: u64, line: u32, from: Option<NonZeroU32> },
    /// The task running (its task_struct): the runtime's, where a recording
    /// begins (no switch says so).
    Task { task: u32 },
    /// A comparison that decides a branch (or a select): `lhs` with `rhs`
    /// (`wide`: 64 bits, else 32), by `pred` (an LLVM icmp predicate, from
    /// eq: see Pred), on `line` (as a Store's).
    Compare { lhs: u64, rhs: u64, wide: bool, pred: u8, line: u32 },
    /// Variable `var` of the function running (Tables::vars: a named one
    /// kept in a wasm local) takes `value` here, on `line` (as a Store's).
    Var { var: u32, line: u32, value: u64 },
}

/// A Compare's predicate, as LLVM numbers them from eq.
pub(crate) const PREDICATES: [(&str, bool); 10] =
    [("==", false), ("!=", false), (">", false), (">=", false), ("<", false), ("<=", false), (">", true), (">=", true), ("<", true), ("<=", true)];

impl Record {
    /// A Compare's normalized operands, operator, signedness, and result.
    pub(crate) fn comparison(&self) -> Option<(i128, &'static str, i128, bool, bool)> {
        let Record::Compare { lhs, rhs, wide, pred, .. } = *self else { return None };
        let (op, signed) = *PREDICATES.get(pred as usize)?;
        let n = |x: u64| match (signed, wide) {
            (true, true) => x as i64 as i128,
            (true, false) => x as u32 as i32 as i128,
            (false, _) => x as i128,
        };
        let (lhs, rhs) = (n(lhs), n(rhs));
        let held = match op { "==" => lhs == rhs, "!=" => lhs != rhs, ">" => lhs > rhs, ">=" => lhs >= rhs, "<" => lhs < rhs, _ => lhs <= rhs };
        Some((lhs, op, rhs, signed, held))
    }
}

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
    pub(crate) fn get(&self, first: usize) -> Option<(&[u32], u32)> {
        let k = self.at.binary_search_by_key(&(first as u32), |a| a.0).ok()?;
        let end = self.at.get(k + 1).map_or(self.words.len(), |a| a.1 as usize);
        let (&frame, args) = self.words[self.at[k].1 as usize..end].split_last()?;
        Some((args, frame))
    }
}

/// The ring at `trace`, decoded, with each record's id: the number of the
/// pair it starts at, counting every pair ever written (so an id stays the
/// same while the ring goes on; indexes do not); and the calls' arguments.
pub fn ring(mem: &dyn Memory, trace: u32) -> (Vec<Record>, Vec<u32>, Args) {
    // The ring's current contents: `len` pairs from pair `first` (of
    // `words`, its size, as its header says).
    let words = mem.read_u32(trace + wire::WORDS);
    if words < 2 {
        return Default::default();
    }
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
pub(crate) struct Decoder<F> {
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

    /// The continuation pairs from the next one, into `values`: how many.
    fn read_values(&mut self) -> usize {
        self.values.clear();
        while self.next < self.len {
            let (value, t) = (self.pair)(self.next);
            if Kind::of(t) != Some(Kind::Continuation) {
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
            let (payload, tag) = (self.pair)(self.next);
            self.head = self.next;
            self.next += 1;
            let got = self.read_values();
            let v = [self.values.first().copied().unwrap_or(0), self.values.get(1).copied().unwrap_or(0)];
            let fields = wire::fields(tag);
            match Kind::of(tag) {
                Some(Kind::Block) => {
                    // (An entry block: the call's arguments, as many words as
                    // they take, then its frame base.)
                    let n = fields;
                    if (n == 0 && !(1..=15).contains(&got)) || (n != 0 && got != 0) {
                        continue;
                    }
                    return Some(Record::Block { func: payload, n });
                }
                Some(Kind::Task) if got == 0 => return Some(Record::Task { task: payload }),
                Some(Kind::Return) if matches!(got, 1 | 2) => {
                    let value = if got == 2 { (v[0] as u64 | (v[1] as u64) << 32) as i64 } else { v[0] as i32 as i64 };
                    return Some(Record::Return { func: payload, value });
                }
                Some(Kind::Compare) if matches!(got, 1 | 3) => {
                    // (A wide one: then the high words, lhs's and rhs's.)
                    let wide = got == 3;
                    let hi = |k: usize| if wide { (self.values[k] as u64) << 32 } else { 0 };
                    return Some(Record::Compare { lhs: payload as u64 | hi(1), rhs: v[0] as u64 | hi(2), wide, pred: (fields & 0xf) as u8, line: fields >> 4 });
                }
                Some(Kind::Store) => {
                    // (The old value; then, a copy's, where from.)
                    let Some(size) = StoreSize::of(fields & 0xf) else { continue };
                    let words = size.old_words();
                    if got != words && got != words + 1 {
                        continue;
                    }
                    let from = self.values.get(words).copied().and_then(NonZeroU32::new);
                    let old = v[0] as u64 | if size == StoreSize::B8 { (v[1] as u64) << 32 } else { 0 };
                    return Some(Record::Store { addr: payload, size: size.bytes(), old, line: fields >> 4, from });
                }
                // (A wide one's high word after it.)
                Some(Kind::Var) if got <= 1 => return Some(Record::Var { var: fields & 0xffff, line: fields >> 16, value: payload as u64 | (v[0] as u64) << 32 }),
                _ => continue, // malformed, unknown, or a continuation without its head
            }
        }
        None
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const BLOCK: u32 = Kind::Block as u32;
    const RETURN: u32 = Kind::Return as u32;
    const COMPARE: u32 = Kind::Compare as u32;
    const STORE: u32 = Kind::Store as u32;
    const VALUE: u32 = Kind::Continuation as u32;
    const VAR: u32 = Kind::Var as u32;
    const TASK: u32 = Kind::Task as u32;

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
    fn a_wide_return_has_its_high_word() {
        let w = [5, RETURN << 28, 1, VALUE << 28, 2, VALUE << 28, 6, RETURN << 28, -2i32 as u32, VALUE << 28];
        assert_eq!(decode(&w), [Record::Return { func: 5, value: 1 | 2 << 32 }, Record::Return { func: 6, value: -2 }]);
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
        let mut w = alloc::vec![5, BLOCK << 28, 1, VALUE << 28, 2, VALUE << 28, 0x900, VALUE << 28];
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
        // (The store of 4 at 0x100 is on its block's line 3; an invalid
        // store size class is skipped without consuming the next head.)
        let w = [9, BLOCK << 28, 7, VALUE << 28, 8, VALUE << 28, 0x80, VALUE << 28, 0x100, STORE << 28 | 3 << 4 | 2, 5, VALUE << 28,
                 0x200, STORE << 28 | 7, 64, VALUE << 28, 9, BLOCK << 28 | 1];
        assert_eq!(decode_values(&w)[0].1, [7, 8, 0x80]);
        assert_eq!(decode(&w), [Record::Block { func: 9, n: 0 },
                                Record::Store { addr: 0x100, size: 4, old: 5, line: 3, from: None },
                                Record::Block { func: 9, n: 1 }]);
    }

    #[test]
    fn copies_and_comparisons() {
        // A store of 4 bytes at 0x100 (it held 5) copied from 0x300; then 18 >= 0 (sge, 7), on line 2.
        let w = [0x100, STORE << 28 | 2, 5, VALUE << 28, 0x300, VALUE << 28, 18, COMPARE << 28 | 2 << 4 | 7, 0, VALUE << 28];
        let got = decode(&w);
        assert_eq!(got, [Record::Store { addr: 0x100, size: 4, old: 5, line: 0, from: NonZeroU32::new(0x300) }, Record::Compare { lhs: 18, rhs: 0, wide: false, pred: 7, line: 2 }]);
        assert_eq!(got[1].comparison(), Some((18, ">=", 0, true, true)));
        assert_eq!(Record::Compare { lhs: -1i32 as u32 as u64, rhs: 0, wide: false, pred: 3, line: 0 }.comparison(),
                   Some((u32::MAX as i128, ">=", 0, false, true)));
        // (Wide: 2^32 > 1 (sgt, 6) holds; their low halves, 0 > 1, would not.)
        let w = [0, COMPARE << 28 | 6, 1, VALUE << 28, 1, VALUE << 28, 0, VALUE << 28];
        assert_eq!(decode(&w)[0].comparison(), Some((1i128 << 32, ">", 1, true, true)));
        assert_eq!(Record::Compare { lhs: 5, rhs: 5, wide: false, pred: 1, line: 0 }.comparison(), Some((5, "!=", 5, false, false)));
        assert_eq!(Record::Compare { lhs: u32::MAX as u64, rhs: 0, wide: false, pred: 6, line: 0 }.comparison(), Some((-1, ">", 0, true, false)));
        assert_eq!(Record::Compare { lhs: u64::MAX, rhs: 0, wide: true, pred: 8, line: 0 }.comparison(), Some((-1, "<", 0, true, true)));
        assert_eq!(Record::Compare { lhs: u64::MAX, rhs: 0, wide: true, pred: 2, line: 0 }.comparison(), Some((u64::MAX as i128, ">", 0, false, true)));
    }

    #[test]
    fn stores_and_tasks_as_ksee_c_writes_them() {
        // (A 4-byte store on line 5; an 8-byte copy from 0x200: its old value
        // low word first; a Task record.)
        let b4 = [0x100, STORE << 28 | 5 << 4 | 2, 0x1234, VALUE << 28];
        let b8 = [0x100, STORE << 28 | 5 << 4 | 3, 0x5566_7788, VALUE << 28, 0x1122_3344, VALUE << 28, 0x200, VALUE << 28];
        assert_eq!(decode(&b4), [Record::Store { addr: 0x100, size: 4, old: 0x1234, line: 5, from: None }]);
        assert_eq!(decode(&b8), [Record::Store { addr: 0x100, size: 8, old: 0x1122_3344_5566_7788, line: 5, from: NonZeroU32::new(0x200) }]);
        assert_eq!(decode(&[0x300, TASK << 28]), [Record::Task { task: 0x300 }]);
    }

    #[test]
    fn malformed_cardinalities_do_not_consume_the_next_head() {
        let bad = [
            (TASK << 28 | 0, 1),
            (BLOCK << 28 | 1, 1),
            (BLOCK << 28 | 0, 0),
            (BLOCK << 28 | 0, 16),
            (RETURN << 28 | 0, 0),
            (RETURN << 28 | 0, 3),
            (COMPARE << 28 | 0, 0),
            (COMPARE << 28 | 0, 2),
            (STORE << 28 | StoreSize::B4 as u32, 0),
            (STORE << 28 | StoreSize::B4 as u32, 3),
            (VAR << 28 | 0, 2),
        ];
        let mut words = Vec::new();
        for (tag, n) in bad {
            words.extend([0, tag]);
            for value in 0..n {
                words.extend([value, VALUE << 28]);
            }
            words.extend(block(9, 1));
        }
        assert_eq!(decode(&words), alloc::vec![Record::Block { func: 9, n: 1 }; bad.len()]);
    }

    #[test]
    fn a_call_has_arguments_and_a_frame_base() {
        for n in [1, 15] {
            let mut words = alloc::vec![9, BLOCK << 28];
            for value in 0..n {
                words.extend([value, VALUE << 28]);
            }
            assert_eq!(decode_values(&words), [(Record::Block { func: 9, n: 0 }, (0..n).collect())]);
        }
    }

    #[test]
    fn reads_a_wrapped_ring() {
        // A 4-word ring after 6 words (blocks 0, 1, 2) were written.
        let mut m = alloc::vec![0u8; 32];
        m[0..4].copy_from_slice(&6u32.to_le_bytes());
        m[4..8].copy_from_slice(&4u32.to_le_bytes());
        let words = [block(9, 2), block(9, 1)].concat();
        for (slot, w) in words.iter().enumerate() {
            let at = RING as usize + slot * 4;
            m[at..at + 4].copy_from_slice(&w.to_le_bytes());
        }
        let (got, ids, _) = ring(&Ring(m), 0);
        assert_eq!(got, [Record::Block { func: 9, n: 1 }, Record::Block { func: 9, n: 2 }]);
        assert_eq!(ids, [1, 2]);
    }
}
