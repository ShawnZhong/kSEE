//! The trace's pairs of words: tags, fields, and records the host writes.

use core::num::NonZeroU32;

const KIND_SHIFT: u32 = 28;
const FIELDS: u32 = (1 << KIND_SHIFT) - 1;
const LINE: u32 = (1 << 24) - 1;
const MAX_WORDS: usize = 8;

/// Offset of the ring's words in the trace header.
pub const RING: u32 = 16;
/// Offset of the `on` word (probes record only while it is nonzero).
pub const ON: u32 = 8;

#[derive(Clone, Copy, PartialEq)]
#[repr(u32)]
pub(super) enum Kind {
    Block = 1,
    Task = 3,
    Return = 4,
    Compare = 6,
    Store = 7,
    Continuation = 8,
    Var = 9,
}

impl Kind {
    pub(super) fn of(tag: u32) -> Option<Kind> {
        Some(match tag >> KIND_SHIFT {
            1 => Kind::Block,
            3 => Kind::Task,
            4 => Kind::Return,
            6 => Kind::Compare,
            7 => Kind::Store,
            8 => Kind::Continuation,
            9 => Kind::Var,
            _ => return None,
        })
    }
}

pub(super) const fn tag(kind: Kind, fields: u32) -> u32 {
    (kind as u32) << KIND_SHIFT | fields & FIELDS
}

pub(super) const fn fields(tag: u32) -> u32 {
    tag & FIELDS
}

/// A Store record's width.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u32)]
pub enum StoreSize {
    B1 = 0,
    B2 = 1,
    B4 = 2,
    B8 = 3,
}

impl StoreSize {
    pub const fn bytes(self) -> u32 {
        1 << self as u32
    }

    pub(super) const fn old_words(self) -> usize {
        if matches!(self, StoreSize::B8) { 2 } else { 1 }
    }

    pub(super) fn of(class: u32) -> Option<StoreSize> {
        Some(match class {
            0 => StoreSize::B1,
            1 => StoreSize::B2,
            2 => StoreSize::B4,
            3 => StoreSize::B8,
            _ => return None,
        })
    }
}

/// A bulk write's chunks. A copy whose higher destination overlaps its
/// source goes backwards, so no earlier synthetic destination is a later
/// chunk's apparent source.
pub struct StoreChunks {
    off: u32,
    len: u32,
    backwards: bool,
}

impl StoreChunks {
    pub fn new(addr: u32, len: u32, from: u32) -> StoreChunks {
        let backwards = from != 0 && from < addr && addr - from < len;
        StoreChunks {
            off: if backwards { len } else { 0 },
            len,
            backwards,
        }
    }
}

impl Iterator for StoreChunks {
    type Item = (u32, StoreSize);

    fn next(&mut self) -> Option<Self::Item> {
        let left = if self.backwards {
            self.off
        } else {
            self.len - self.off
        };
        let size = match left {
            0 => return None,
            1 => StoreSize::B1,
            2..=3 => StoreSize::B2,
            4..=7 => StoreSize::B4,
            _ => StoreSize::B8,
        };
        if self.backwards {
            self.off -= size.bytes();
            Some((self.off, size))
        } else {
            let off = self.off;
            self.off += size.bytes();
            Some((off, size))
        }
    }
}

/// One complete runtime-written record, as an even number of words.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EncodedRecord {
    words: [u32; MAX_WORDS],
    len: u8,
}

impl EncodedRecord {
    fn new(first: u32, kind: Kind, fields: u32) -> EncodedRecord {
        let mut out = EncodedRecord {
            words: [0; MAX_WORDS],
            len: 2,
        };
        (out.words[0], out.words[1]) = (first, tag(kind, fields));
        out
    }

    fn continuation(&mut self, value: u32) {
        let i = self.len as usize;
        (self.words[i], self.words[i + 1]) = (value, tag(Kind::Continuation, 0));
        self.len += 2;
    }

    pub fn words(&self) -> &[u32] {
        &self.words[..self.len as usize]
    }
}

/// A Store record: where, the old value, and, for a copy, where from.
pub fn store(
    addr: u32,
    line: u32,
    size: StoreSize,
    old: u64,
    from: Option<NonZeroU32>,
) -> EncodedRecord {
    let mut out = EncodedRecord::new(addr, Kind::Store, (line & LINE) << 4 | size as u32);
    out.continuation(old as u32);
    if size == StoreSize::B8 {
        out.continuation((old >> 32) as u32);
    }
    if let Some(from) = from {
        out.continuation(from.get());
    }
    out
}

/// A Task record: the task running when a requested recording begins.
pub fn task(task: u32) -> EncodedRecord {
    EncodedRecord::new(task, Kind::Task, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bulk_chunks_follow_the_safe_copy_direction() {
        let chunks = |addr, len, from| {
            StoreChunks::new(addr, len, from)
                .map(|(off, size)| (off, size.bytes()))
                .collect::<alloc::vec::Vec<_>>()
        };
        let forward = alloc::vec![(0, 8), (8, 4), (12, 2), (14, 1)];
        assert_eq!(chunks(100, 15, 0), forward);
        assert_eq!(chunks(100, 15, 200), forward);
        assert_eq!(chunks(100, 15, 101), forward);
        assert_eq!(chunks(115, 15, 100), forward);
        assert_eq!(chunks(101, 15, 100), [(7, 8), (3, 4), (1, 2), (0, 1)]);
    }

    #[test]
    fn runtime_records_have_the_wire_format() {
        assert_eq!(task(9).words(), [9, 3 << 28]);
        assert_eq!(
            store(0x100, 5, StoreSize::B4, 0x1234, None).words(),
            [0x100, 7 << 28 | 5 << 4 | 2, 0x1234, 8 << 28]
        );
        assert_eq!(
            store(
                0x100,
                5,
                StoreSize::B8,
                0x1122_3344_5566_7788,
                NonZeroU32::new(0x200)
            )
            .words(),
            [
                0x100,
                7 << 28 | 5 << 4 | 3,
                0x5566_7788,
                8 << 28,
                0x1122_3344,
                8 << 28,
                0x200,
                8 << 28
            ]
        );
    }
}
