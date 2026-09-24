//! kSEE host core, compiled to wasm and instantiated next to the kernel
//! in every kernel Worker.
//!
//! The kernel's probe imports (`ksee.enter`, `ksee.exit`, `ksee.block`;
//! see plugin/ksee-trace.cpp) are this module's exports of the same names,
//! so a probe is a direct wasm call. This module has its own memory: it
//! reads kernel objects through the `host.kmem_read` import and hands
//! finished records to `host.emit`.

pub mod event;
pub mod tables;

#[cfg(target_arch = "wasm32")]
mod wasm;
