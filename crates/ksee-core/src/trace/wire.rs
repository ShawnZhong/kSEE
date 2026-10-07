//! The trace's records' tags: a kind, a count of values and fields (the
//! plugin and arch/wasm/kernel/ksee.c write them).

const KIND_SHIFT: u32 = 28;
const COUNT_SHIFT: u32 = 24;
const FIELDS: u32 = (1 << COUNT_SHIFT) - 1;

/// Offset of the ring's words in the trace header.
pub const RING: u32 = 12;
/// Offset of the ring's size in words (arch/wasm/kernel/ksee.c).
pub const WORDS: u32 = 4;

#[derive(Clone, Copy, PartialEq)]
#[repr(u32)]
pub(super) enum Kind {
    Block = 1,
    Task = 3,
    Return = 4,
    Compare = 6,
    Store = 7,
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
            9 => Kind::Var,
            _ => return None,
        })
    }
}

pub(super) const fn fields(tag: u32) -> u32 {
    tag & FIELDS
}

/// How many values come before the record's head.
pub(super) const fn count(tag: u32) -> u32 {
    tag >> COUNT_SHIFT & 0xf
}
