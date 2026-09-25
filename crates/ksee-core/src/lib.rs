//! kSEE core: everything about a recording that needs no browser.
//!
//! * [`tables`], [`types`]: what the kernel says about itself (the plugin's
//!   probe tables; types from DWARF, see crates/ksee-cli's pack).
//! * [`trace`]: decoding the trace ring into records.
//! * [`recording`]: what the host rebuilds from the records: tasks,
//!   syscalls ([`syscalls`]), calls and lines ([`stack`]), memory as it was
//!   ([`mem`]).
//! * [`view`]: what the page shows of it.
//!
//! `no_std`, so that the same code runs in the wasm runtime
//! (crates/ksee-runtime) and natively (crates/ksee-cli, tests).
#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

pub mod arch;
pub mod mem;
pub mod module;
pub mod recording;
pub mod stack;
pub mod syscalls;
pub mod tables;
pub mod task;
pub mod trace;
pub mod types;
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
