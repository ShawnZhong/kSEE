//! kSEE host runtime.
//!
//! Compiled to wasm, it runs inside the kernel's shared linear memory,
//! below the kernel image (CONFIG_WASM_HOST_MEMORY), so it reads kernel
//! objects and the trace ring (see plugin/ksee-trace.cpp) directly. The JS
//! shim only does what needs the platform: Workers, compiling and
//! instantiating modules, time and console I/O.
//!
//! The logic is plain `no_std` Rust over a [`Memory`] view, unit-tested
//! natively; `wasm` holds the wasm entry points.
#![cfg_attr(target_arch = "wasm32", no_std)]
#![cfg_attr(target_arch = "wasm32", feature(stdarch_wasm_atomic_wait))]

extern crate alloc;

pub mod json;
pub mod syscalls;
pub mod tables;
pub mod trace;

#[cfg(target_arch = "wasm32")]
mod wasm;

/// Read access to kernel memory.
pub trait Memory {
    fn read(&self, addr: u32, buf: &mut [u8]);

    fn read_u32(&self, addr: u32) -> u32 {
        let mut b = [0; 4];
        self.read(addr, &mut b);
        u32::from_le_bytes(b)
    }
}
