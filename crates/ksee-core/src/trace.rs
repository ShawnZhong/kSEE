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
    /// A return from `func`, with `value` (a 32-bit one sign-extended;
    /// none: a function that returns nothing).
    Return { func: u32, value: Option<i64> },
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
    /// eq: see PREDICATES), on `line` (as a Store's).
    Compare { lhs: u64, rhs: u64, wide: bool, pred: u8, line: u32 },
    /// Variable `var` of the function running (Tables::vars: a named one
    /// kept in a wasm local) takes `value` here, on `line` (as a Store's).
    Var { var: u32, line: u32, value: u64 },
}

/// A Compare's predicate, as LLVM numbers them from eq.
pub(crate) const PREDICATES: [(&str, bool); 10] =
    [("==", false), ("!=", false), (">", false), (">=", false), ("<", false), ("<=", false), (">", true), (">=", true), ("<", true), ("<=", true)];

impl Record {
    /// Which of its block's lines it is on: a store's, a comparison's, a
    /// variable set's (see tables::Block::steps).
    pub(crate) fn line(&self) -> Option<u32> {
        match *self {
            Record::Store { line, .. } | Record::Compare { line, .. } | Record::Var { line, .. } => Some(line),
            _ => None,
        }
    }

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

/// The ring at `trace`, decoded, with each record's id: the word it
/// starts at, counting every word ever written (so an id stays the same
/// while the ring goes on; indexes do not); and the calls' arguments.
pub fn ring(mem: &dyn Memory, trace: u32) -> (Vec<Record>, Vec<u32>, Args) {
    // (The ring's size, as its header says; the words ever written, of
    // which it keeps the last.)
    let words = mem.read_u32(trace + wire::WORDS);
    if words == 0 {
        return Default::default();
    }
    let pos = mem.read_u32(trace);
    let found = decode(pos, pos.saturating_sub(words), |i| mem.read_u32(trace + RING + i % words * 4));
    let (mut records, mut ids, mut args) = (Vec::with_capacity(found.len()), Vec::with_capacity(found.len()), Args::default());
    for (id, r, values) in found {
        if matches!(r, Record::Block { n: 0, .. }) {
            args.push(records.len(), &values);
        }
        records.push(r);
        ids.push(id);
    }
    (records, ids, args)
}

/// The records that end at word `pos`, back to word `oldest` (what the ring
/// still has: a record that starts before it is gone), `word(i)` the
/// word ever written `i`-th: each record's first word, the record, its
/// values; oldest first. A record ends with its head, `a` and its tag,
/// which says how many values come before them: read back from `pos`, a
/// record's end, the records are each whole.
pub(crate) fn decode(pos: u32, oldest: u32, word: impl Fn(u32) -> u32) -> Vec<(u32, Record, Vec<u32>)> {
    let mut out = Vec::new();
    let mut p = pos;
    while p >= oldest.saturating_add(2) {
        let tag = word(p - 1);
        let Some(start) = p.checked_sub(2 + wire::count(tag)).filter(|&s| s >= oldest) else { break };
        let values: Vec<u32> = (start..p - 2).map(&word).collect();
        if let Some(r) = record(word(p - 2), tag, &values) {
            out.push((start, r, values));
        }
        p = start;
    }
    out.reverse();
    out
}

/// The record head `a`, `tag` and its `values` are (none: malformed, or
/// unknown).
fn record(a: u32, tag: u32, v: &[u32]) -> Option<Record> {
    let fields = wire::fields(tag);
    let got = v.len();
    let word = |k: usize| v.get(k).copied().unwrap_or(0) as u64;
    Some(match Kind::of(tag)? {
        // (An entry block: the call's arguments, as many words as they
        // take, then its frame base.)
        Kind::Block if (fields == 0) == (1..=15).contains(&got) => Record::Block { func: a, n: fields },
        Kind::Task if got == 0 => Record::Task { task: a },
        Kind::Return if got <= 2 => {
            let value = match got { 0 => None, 1 => Some(word(0) as u32 as i32 as i64), _ => Some((word(0) | word(1) << 32) as i64) };
            Record::Return { func: a, value }
        }
        // (A wide one: then the high words, lhs's and rhs's.)
        Kind::Compare if matches!(got, 1 | 3) => {
            let wide = got == 3;
            let hi = |k: usize| if wide { word(k) << 32 } else { 0 };
            Record::Compare { lhs: a as u64 | hi(1), rhs: word(0) | hi(2), wide, pred: (fields & 0xf) as u8, line: fields >> 4 }
        }
        // (The old value; then, a copy's, where from.)
        Kind::Store => {
            let size = StoreSize::of(fields & 0xf)?;
            let words = size.old_words();
            if got != words && got != words + 1 {
                return None;
            }
            let from = v.get(words).copied().and_then(NonZeroU32::new);
            let old = word(0) | if size == StoreSize::B8 { word(1) << 32 } else { 0 };
            Record::Store { addr: a, size: size.bytes(), old, line: fields >> 4, from }
        }
        // (A wide one's high word before it.)
        Kind::Var if got <= 1 => Record::Var { var: fields & 0xfff, line: fields >> 12, value: a as u64 | word(0) << 32 },
        _ => return None,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const BLOCK: u32 = Kind::Block as u32;
    const RETURN: u32 = Kind::Return as u32;
    const COMPARE: u32 = Kind::Compare as u32;
    const STORE: u32 = Kind::Store as u32;
    const VAR: u32 = Kind::Var as u32;
    const TASK: u32 = Kind::Task as u32;

    /// A record as ksee.c writes it: its values, then `a` and its tag
    /// (`kind` and `fields`) with their count.
    pub fn rec(a: u32, kind: u32, fields: u32, values: &[u32]) -> Vec<u32> {
        let mut w = values.to_vec();
        w.extend([a, kind << 28 | (values.len() as u32) << 24 | fields]);
        w
    }

    /// Records in `words`, oldest first, read back from their end.
    fn decode(words: &[u32]) -> Vec<Record> {
        decode_values(words).into_iter().map(|(r, _)| r).collect()
    }

    /// The same, with each record's values.
    fn decode_values(words: &[u32]) -> Vec<(Record, Vec<u32>)> {
        super::decode(words.len() as u32, 0, |i| words[i as usize]).into_iter().map(|(_, r, v)| (r, v)).collect()
    }

    #[test]
    fn a_wide_return_has_its_high_word() {
        let w = [rec(5, RETURN, 0, &[1, 2]), rec(6, RETURN, 0, &[-2i32 as u32])].concat();
        assert_eq!(decode(&w), [Record::Return { func: 5, value: Some(1 | 2 << 32) }, Record::Return { func: 6, value: Some(-2) }]);
    }

    #[test]
    fn a_variable_takes_a_value() {
        // (A wide one: its high word with it.)
        let w = [rec(5, VAR, 2, &[]), rec(9, VAR, 4 << 12 | 3, &[1])].concat();
        assert_eq!(decode(&w), [Record::Var { var: 2, line: 0, value: 5 }, Record::Var { var: 3, line: 4, value: 9 | 1 << 32 }]);
    }

    pub fn block(func: u32, n: u32) -> Vec<u32> {
        rec(func, BLOCK, n, &[])
    }

    pub fn ret(func: u32, value: u32) -> Vec<u32> {
        rec(func, RETURN, 0, &[value])
    }

    #[test]
    fn a_record_cut_by_the_ring_is_gone_whole() {
        // (A call: block 0, arguments 1 and 2, the frame base 0x900.)
        let mut w = rec(5, BLOCK, 0, &[1, 2, 0x900]);
        w.extend(block(5, 3));
        w.extend(ret(5, 42));
        assert_eq!(decode_values(&w)[0], (Record::Block { func: 5, n: 0 }, alloc::vec![1, 2, 0x900]));
        // (Its first word overwritten: the ring has from word 1 on.)
        let got: Vec<Record> = super::decode(w.len() as u32, 1, |i| w[i as usize]).into_iter().map(|(_, r, _)| r).collect();
        assert_eq!(got, [Record::Block { func: 5, n: 3 }, Record::Return { func: 5, value: Some(42) }]);
    }


    #[test]
    fn calls_carry_arguments_and_stores_old_values() {
        // (The store of 4 at 0x100 is on its block's line 3; an invalid
        // store size class is skipped, the records before it read.)
        let w = [rec(9, BLOCK, 0, &[7, 8, 0x80]), rec(0x100, STORE, 3 << 4 | 2, &[5]), rec(0x200, STORE, 7, &[64]), block(9, 1)].concat();
        assert_eq!(decode_values(&w)[0].1, [7, 8, 0x80]);
        assert_eq!(decode(&w), [Record::Block { func: 9, n: 0 },
                                Record::Store { addr: 0x100, size: 4, old: 5, line: 3, from: None },
                                Record::Block { func: 9, n: 1 }]);
    }

    #[test]
    fn copies_and_comparisons() {
        // A store of 4 bytes at 0x100 (it held 5) copied from 0x300; then 18 >= 0 (sge, 7), on line 2.
        let w = [rec(0x100, STORE, 2, &[5, 0x300]), rec(18, COMPARE, 2 << 4 | 7, &[0])].concat();
        let got = decode(&w);
        assert_eq!(got, [Record::Store { addr: 0x100, size: 4, old: 5, line: 0, from: NonZeroU32::new(0x300) }, Record::Compare { lhs: 18, rhs: 0, wide: false, pred: 7, line: 2 }]);
        assert_eq!(got[1].comparison(), Some((18, ">=", 0, true, true)));
        assert_eq!(Record::Compare { lhs: -1i32 as u32 as u64, rhs: 0, wide: false, pred: 3, line: 0 }.comparison(),
                   Some((u32::MAX as i128, ">=", 0, false, true)));
        // (Wide: 2^32 > 1 (sgt, 6) holds; their low halves, 0 > 1, would not.)
        let w = rec(0, COMPARE, 6, &[1, 1, 0]);
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
        let b4 = rec(0x100, STORE, 5 << 4 | 2, &[0x1234]);
        let b8 = rec(0x100, STORE, 5 << 4 | 3, &[0x5566_7788, 0x1122_3344, 0x200]);
        assert_eq!(decode(&b4), [Record::Store { addr: 0x100, size: 4, old: 0x1234, line: 5, from: None }]);
        assert_eq!(decode(&b8), [Record::Store { addr: 0x100, size: 8, old: 0x1122_3344_5566_7788, line: 5, from: NonZeroU32::new(0x200) }]);
        assert_eq!(decode(&rec(0x300, TASK, 0, &[])), [Record::Task { task: 0x300 }]);
    }

    #[test]
    fn malformed_cardinalities_are_skipped_whole() {
        let bad = [(TASK, 0, 1), (BLOCK, 1, 1), (BLOCK, 0, 0), (RETURN, 0, 3), (COMPARE, 0, 0), (COMPARE, 0, 2),
                   (STORE, StoreSize::B4 as u32, 0), (STORE, StoreSize::B4 as u32, 3), (VAR, 0, 2)];
        let mut words = Vec::new();
        for (kind, fields, n) in bad {
            words.extend(rec(0, kind, fields, &(0..n).collect::<Vec<u32>>()));
            words.extend(block(9, 1));
        }
        assert_eq!(decode(&words), alloc::vec![Record::Block { func: 9, n: 1 }; bad.len()]);
    }

    #[test]
    fn a_call_has_arguments_and_a_frame_base() {
        for n in [1, 15] {
            let values: Vec<u32> = (0..n).collect();
            assert_eq!(decode_values(&rec(9, BLOCK, 0, &values)), [(Record::Block { func: 9, n: 0 }, values)]);
        }
    }

    #[test]
    fn reads_a_wrapped_ring() {
        // A 5-word ring after 7 words were written: a store (3 words), then
        // blocks 1 and 2; the store's first words overwritten.
        let words = [rec(0x100, STORE, 2, &[5]), block(9, 1), block(9, 2)].concat();
        let mut m = alloc::vec![0u8; RING as usize + 20];
        m[0..4].copy_from_slice(&7u32.to_le_bytes());
        m[4..8].copy_from_slice(&5u32.to_le_bytes());
        for (i, w) in words.iter().enumerate() {
            let at = RING as usize + i % 5 * 4;
            m[at..at + 4].copy_from_slice(&w.to_le_bytes());
        }
        let (got, ids, _) = ring(&m, 0);
        assert_eq!(got, [Record::Block { func: 9, n: 1 }, Record::Block { func: 9, n: 2 }]);
        assert_eq!(ids, [3, 5]);
    }
}
