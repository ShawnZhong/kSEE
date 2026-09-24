//! kSEE host runtime.
//!
//! A wasm-bindgen module built with atomics: every Worker instantiates it
//! on the kernel's shared memory (wasm-bindgen gives each instance its own
//! stack and thread-locals; std's heap grows the memory above the kernel's
//! RAM). Its static data sits below the kernel image
//! (CONFIG_WASM_HOST_MEMORY), so it reads kernel objects and the trace
//! ring directly.
//!
//! * [`machine`]: the page's API: boot a kernel, console, views.
//! * [`worker`]: what each Worker runs: the timer, or a kernel task.
//! * [`imports`]: the kernel's imports. The frequent ones are this
//!   module's exports (direct wasm calls); the rest reach Web APIs.
//! * [`debug`]: breakpoints and stepping (stopping the kernel in a probe).
//! * [`state`]: statics shared by all instances.

#![feature(stdarch_wasm_atomic_wait)]

mod debug;
mod imports;
mod js;
pub mod machine;
mod state;
mod worker;
