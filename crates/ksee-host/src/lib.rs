//! kSEE host runtime: a wasm-bindgen module on the page's thread.
//!
//! The kernel is one wasm instance on this module's memory (its RAM above
//! the runtime's data and stack, CONFIG_WASM_HOST_MEMORY; the runtime's
//! heap above its RAM), so the runtime reads kernel objects and the trace
//! ring directly. Its tasks are call stacks of that instance, which JSPI
//! suspends and resumes (sched).
//!
//! * [`machine`]: the page's API: boot a kernel, drive it, snapshot its
//!   recording; [`replay`]: its views of the recording (ksee_core::view).
//! * [`sched`]: the kernel's tasks, on one thread.
//! * [`imports`]: the kernel's imports. The frequent ones are this
//!   module's exports (direct wasm calls); the rest reach Web APIs.
//! * [`programs`]: user programs (in kernel memory), their tasks' stack
//!   pointers, and the runners that make the page's calls.
//! * [`state`]: the machine's state in the runtime.

mod imports;
mod js;
pub mod machine;
mod programs;
mod replay;
mod sched;
mod state;

pub use state::Finished;
