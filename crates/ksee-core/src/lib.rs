//! kSEE core: everything about a recording that needs no browser.
//!
//! * [`tables`], [`types`]: what the kernel says about itself (the plugin's
//!   probe tables; types from DWARF, see crates/ksee-cli's pack), the big
//!   ones [`flat`], read in place; [`wasm`]:
//!   where they are, a module's custom sections; [`arch`]: the kernel's own
//!   names, and its timers.
//! * [`trace`]: decoding the trace ring into records.
//! * [`recording`]: what the host rebuilds from the records: tasks,
//!   syscalls, and the calls and lines ([`stack`]), memory as it was
//!   ([`mem`]).
//! * [`view`]: what the page shows of it.
//! * [`program`]: the page's program: a line's C (with the kernel's types),
//!   and the run of it (program::Session: its runners and calls).
//! * [`highlight`]: the kernel's source, colored.
//! * [`text`]: a lesson's paragraphs, in pieces.
//!
//! `no_std`, so that the same code runs in the wasm runtime
//! (crates/ksee-host) and natively (its tests, on made-up data).
#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

pub mod arch;
pub mod flat;
pub mod highlight;
pub mod program;
pub mod mem;
pub mod recording;
pub mod stack;
pub mod tables;
pub mod text;
pub mod trace;
pub mod types;
pub mod view;
pub mod wasm;

/// Read access to memory: the kernel's (or bytes anywhere: see wasm).
pub trait Memory {
    fn read(&self, addr: u32, buf: &mut [u8]);

    fn read_u8(&self, addr: u32) -> u8 {
        let mut b = [0; 1];
        self.read(addr, &mut b);
        b[0]
    }

    fn read_u32(&self, addr: u32) -> u32 {
        let mut b = [0; 4];
        self.read(addr, &mut b);
        u32::from_le_bytes(b)
    }
}

/// Bytes as memory, each at its index (a module's, a test's): past their
/// end, zeros.
impl Memory for &[u8] {
    fn read(&self, addr: u32, buf: &mut [u8]) {
        let at = (addr as usize).min(self.len());
        let n = buf.len().min(self.len() - at);
        buf[..n].copy_from_slice(&self[at..at + n]);
        buf[n..].fill(0);
    }
}

impl Memory for alloc::vec::Vec<u8> {
    fn read(&self, addr: u32, buf: &mut [u8]) {
        (&self[..]).read(addr, buf)
    }
}
