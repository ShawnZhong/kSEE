//! kSEE host runtime.
//!
//! A wasm-bindgen module built with atomics: every Worker instantiates it
//! on the kernel's shared memory (wasm-bindgen gives each instance its own
//! stack and thread-locals; std's heap grows the memory above the kernel's
//! RAM). Its static data sits below the kernel image
//! (CONFIG_WASM_HOST_MEMORY), so it reads kernel objects and the trace
//! ring directly.
//!
//! The page runs:
//! * [`machine`]: its API: boot a kernel, drive it, snapshot its recording;
//! * [`replay`]: its views of the recording (ksee_core::view, as JS values).
//!
//! The kernel's Workers run:
//! * [`worker`]: the timer, or a kernel task;
//! * [`imports`]: the kernel's imports. The frequent ones are this
//!   module's exports (direct wasm calls); the rest reach Web APIs.
//!
//! Both share [`state`]: statics in the shared memory.

#![feature(stdarch_wasm_atomic_wait)]

mod imports;
mod js;
pub mod machine;
mod replay;
mod state;
mod worker;
