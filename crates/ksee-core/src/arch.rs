//! The kernel functions the host rebuilds a recording by (arch/wasm's, and
//! the scheduler's): the only kernel names ksee-core knows.

/// A syscall (arch/wasm/kernel/syscall.c): its call's arguments are the
/// number and the syscall's; its return's value is the result.
pub const SYSCALL: &str = "wasm_syscall";
/// A syscall's table entry (arch/wasm/include/asm/syscall_wrapper.h:
/// `__wasm_sys_read`), the one function SYSCALL_DEFINE makes for it, the
/// kernel's generic shims inlined into it at its own line: shown as part
/// of the entry, one frame, named for the syscall.
pub const SYSCALL_WRAPPER: &str = "__wasm_sys_";
/// The machine's own primitives (arch/wasm/include/asm and the generic
/// half of the arch layer: `current`, the interrupt flag, atomics, bit
/// ops), inlined all over the kernel: the machine, not calls. Their code
/// counts as the line it is inlined at.
pub const PRIMITIVES: [&str; 2] = ["arch/wasm/include/asm/", "include/asm-generic/"];
/// Where a task starts (arch/wasm/kernel/process.c), with an empty stack.
pub const TASK_ENTRY: &str = "wasm_task_entry";
/// The context switch: switch(prev, next) returns on next's stack.
pub const SWITCH: &str = "__switch_to";
