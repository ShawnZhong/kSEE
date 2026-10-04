//! The trace's pairs of words: tags and fields (the plugin and
//! arch/wasm/kernel/ksee.c write them).


const KIND_SHIFT: u32 = 28;
const FIELDS: u32 = (1 << KIND_SHIFT) - 1;

/// Offset of the ring's words in the trace header.
pub const RING: u32 = 16;
/// Offset of the ring's size in words (arch/wasm/kernel/ksee.c).
pub const WORDS: u32 = 4;
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
