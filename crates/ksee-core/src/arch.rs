//! The kernel functions the host rebuilds a recording by (arch/wasm's, and
//! the scheduler's): the only kernel names ksee-core knows.

/// A syscall (arch/wasm/kernel/syscall.c): its call's arguments are the
/// number and the syscall's; its return's value is the result.
pub const SYSCALL: &str = "wasm_syscall";
/// The wrappers between the syscall entry and a syscall's own code (the
/// arch's table entry, then SYSCALL_DEFINE's): shown as part of the entry,
/// one frame, named for the syscall.
pub const SYSCALL_WRAPPERS: &[&str] = &["__wasm_sys_", "__se_sys_", "__do_sys_"];
/// Where a task starts (arch/wasm/kernel/process.c), with an empty stack.
pub const TASK_ENTRY: &str = "wasm_task_entry";
/// The context switch: switch(prev, next) returns on next's stack.
pub const SWITCH: &str = "__switch_to";
