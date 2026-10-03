//! kSEE core: everything about a recording that needs no browser.
//!
//! * [`tables`], [`types`]: what the kernel says about itself (the plugin's
//!   probe tables; types from DWARF, see crates/ksee-cli's pack).
//! * [`trace`]: decoding the trace ring into records.
//! * [`recording`]: what the host rebuilds from the records: tasks,
//!   syscalls, and the calls and lines ([`stack`]), memory as it was
//!   ([`mem`]).
//! * [`view`]: what the page shows of it.
//! * [`program`]: a syscall's arguments as the page writes them (C, with the
//!   kernel's types).
//! * [`highlight`]: the kernel's source, colored.
//! * [`text`]: a lesson's paragraphs, in pieces.
//!
//! `no_std`, so that the same code runs in the wasm runtime
//! (crates/ksee-runtime) and natively (crates/ksee-cli, tests).
#![no_std]
#![cfg_attr(not(feature = "full"), allow(dead_code))]

extern crate alloc;
#[cfg(test)]
extern crate std;

pub mod arch;
#[cfg(feature = "full")]
pub mod highlight;
pub mod program;
#[cfg(feature = "full")]
pub mod mem;
pub mod module;
#[cfg(feature = "full")]
pub mod recording;
#[cfg(feature = "full")]
pub mod stack;
pub mod tables;
#[cfg(feature = "full")]
pub mod text;
#[cfg(feature = "full")]
pub mod trace;
pub mod types;
#[cfg(feature = "full")]
pub mod view;

/// Read access to kernel memory.
pub trait Memory {
    fn read(&self, addr: u32, buf: &mut [u8]);

    fn read_u32(&self, addr: u32) -> u32 {
        let mut b = [0; 4];
        self.read(addr, &mut b);
        u32::from_le_bytes(b)
    }
}
