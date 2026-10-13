//! Flat tables: slices of plain records, and the text their names are in.
//! The tables file is these, each where the file has it, read in place;
//! parsing makes the same, kept (as the tables are) for the page's life.
//!
//! The file: sections one after another, each its length (8 bytes) and
//! its bytes, padded to 8; read in the order they were written.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

pub use bytemuck::{Pod, Zeroable};

/// A name: where it is in its tables' text.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Pod, Zeroable)]
pub struct Str {
    at: u32,
    len: u32,
}

impl Str {
    pub fn of(self, text: &'static str) -> &'static str {
        text.get(self.at as usize..(self.at + self.len) as usize).unwrap_or("")
    }
}

/// Items `start..end` of a list of lists, the one of `key`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct At {
    pub key: u32,
    pub start: u32,
    pub end: u32,
}

/// The items of `key`, of lists `index` (sorted by key) into `items`.
pub fn list<T>(index: &[At], items: &'static [T], key: u32) -> &'static [T] {
    index.binary_search_by_key(&key, |a| a.key).map_or(&[], |k| &items[index[k].start as usize..index[k].end as usize])
}

/// Lists being made: each pushed whole, its key next (in order, for list).
pub struct Lists<T> {
    pub index: Vec<At>,
    pub items: Vec<T>,
}

impl<T> Default for Lists<T> {
    fn default() -> Self {
        Lists { index: Vec::new(), items: Vec::new() }
    }
}

impl<T> Lists<T> {
    pub fn push(&mut self, key: u32, items: impl IntoIterator<Item = T>) {
        let start = self.items.len() as u32;
        self.items.extend(items);
        self.index.push(At { key, start, end: self.items.len() as u32 });
    }

    /// (index, items), kept; the index sorted by key.
    pub fn keep(mut self) -> (&'static [At], &'static [T]) {
        self.index.sort_by_key(|a| a.key);
        (keep(self.index), keep(self.items))
    }
}

/// `v`, kept for the page's life.
pub fn keep<T>(v: Vec<T>) -> &'static [T] {
    alloc::boxed::Box::leak(v.into_boxed_slice())
}

/// Text being made: each name in it once.
#[derive(Default)]
pub struct Text {
    text: String,
    names: BTreeMap<String, Str>,
}

impl Text {
    pub fn name(&mut self, s: &str) -> Str {
        if let Some(&n) = self.names.get(s) {
            return n;
        }
        let n = Str { at: self.text.len() as u32, len: s.len() as u32 };
        self.text.push_str(s);
        self.names.insert(s.into(), n);
        n
    }

    /// The text, kept.
    pub fn keep(self) -> &'static str {
        alloc::boxed::Box::leak(self.text.into_boxed_str())
    }
}

/// `len` bytes, kept, where records can be read in place (8-aligned):
/// `fill` writes them.
pub fn kept(len: usize, fill: impl FnOnce(&mut [u8])) -> &'static [u8] {
    let words: &'static mut [u64] = alloc::boxed::Box::leak(alloc::vec![0u64; len.div_ceil(8)].into_boxed_slice());
    let bytes = &mut bytemuck::cast_slice_mut::<u64, u8>(words)[..len];
    fill(bytes);
    bytes
}

/// A tables file being written.
#[derive(Default)]
pub struct Writer(Vec<u8>);

impl Writer {
    pub fn bytes(&mut self, b: &[u8]) {
        self.0.extend_from_slice(&(b.len() as u64).to_le_bytes());
        self.0.extend_from_slice(b);
        self.0.resize(self.0.len().next_multiple_of(8), 0);
    }

    pub fn slice<T: Pod>(&mut self, s: &[T]) {
        self.bytes(bytemuck::cast_slice(s));
    }

    pub fn done(self) -> Vec<u8> {
        self.0
    }
}

/// A tables file being read, in place (its bytes 8-aligned: see kept).
pub struct Reader(&'static [u8]);

impl Reader {
    pub fn new(bytes: &'static [u8]) -> Reader {
        Reader(bytes)
    }

    pub fn bytes(&mut self) -> Option<&'static [u8]> {
        let len = u64::from_le_bytes(self.0.get(..8)?.try_into().ok()?) as usize;
        let b = self.0.get(8..8 + len)?;
        self.0 = self.0.get((8 + len).next_multiple_of(8)..).unwrap_or(&[]);
        Some(b)
    }

    pub fn slice<T: Pod>(&mut self) -> Option<&'static [T]> {
        bytemuck::try_cast_slice(self.bytes()?).ok()
    }

    pub fn text(&mut self) -> Option<&'static str> {
        core::str::from_utf8(self.bytes()?).ok()
    }
}
