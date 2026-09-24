//! kSEE core: the kernel's probe tables, trace decoding and views of the
//! trace. `no_std` so that the same code runs in the wasm host runtime
//! (crates/ksee-runtime) and natively (crates/ksee-cli, tests).
#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

pub mod cfg;
pub mod dump;
pub mod module;
pub mod render;
pub mod syscalls;
pub mod tables;
pub mod trace;

/// Read access to kernel memory.
pub trait Memory {
    fn read(&self, addr: u32, buf: &mut [u8]);

    fn read_u32(&self, addr: u32) -> u32 {
        let mut b = [0; 4];
        self.read(addr, &mut b);
        u32::from_le_bytes(b)
    }
}
