//! The kernel's names the host rebuilds and shows a recording by (arch/wasm's
//! functions, the scheduler's, and a few of its types and conventions): the
//! only kernel names ksee-core knows.

/// A syscall (arch/wasm/kernel/syscall.c): its call's arguments are the
/// number and the syscall's; its return's value is the result.
pub const SYSCALL: &str = "wasm_syscall";
/// A syscall's table entry (arch/wasm/include/asm/syscall_wrapper.h:
/// `__wasm_sys_read`), the one function SYSCALL_DEFINE makes for it: its
/// source's parameters (the SYSCALL_DEFINE line's) are the syscall's
/// arguments, on the entry's record.
pub const SYSCALL_WRAPPER: &str = "__wasm_sys_";
/// The machine's own primitives (arch/wasm/include/asm and the generic
/// half of the arch layer: `current`, the interrupt flag, atomics, bit
/// ops), inlined all over the kernel: the machine, not calls. Their code
/// counts as the line it is inlined at.
pub const PRIMITIVES: [&str; 2] = ["arch/wasm/include/asm/", "include/asm-generic/"];
/// The machine's own variables (arch/wasm: the interrupt flag, the task
/// running, the timer), written by every lock and switch: the machine's
/// state, not the kernel's.
pub const MACHINE: &str = "arch/wasm/";
/// Where a task starts (arch/wasm/kernel/process.c), with an empty stack.
pub const TASK_ENTRY: &str = "wasm_task_entry";
/// The context switch: switch(prev, next) returns on next's stack.
pub const SWITCH: &str = "__switch_to";
/// A task (what the task running points at), and the fields of it the
/// Stack shows first.
pub const TASK: &str = "task_struct";
pub const TASK_FIELDS: [&str; 12] = ["pid", "tgid", "comm", "__state", "flags", "prio", "mm", "files", "fs", "real_parent", "group_leader", "signal"];
/// The registers a syscall entry saves (its wrappers point at them).
pub const REGS: &str = "struct pt_regs";
/// A syscall's body (SYSCALL_DEFINE's `__do_sys_NAME`), whose parameters
/// are the syscall's; a `_time64` syscall's is the plain one's.
pub const SYSCALL_BODY: &str = "__do_sys_";
pub const TIME64: &str = "_time64";
/// The constants that fit a syscall's parameter, by the starts of their
/// names: for "syscall.param", else any syscall's parameter of that name.
pub const SYSCALL_CONSTS: &[(&str, &[&str])] = &[
    ("mmap2.flags", &["MAP_"]), ("openat.flags", &["O_"]), ("unlinkat.flag", &["AT_"]), ("renameat2.flags", &["RENAME_"]),
    ("waitid.which", &["P_"]), ("waitid.options", &["W"]), ("clone.clone_flags", &["CLONE_", "SIG"]),
    ("futex_time64.op", &["FUTEX_"]), ("clock_nanosleep_time64.which_clock", &["CLOCK_"]), ("clock_nanosleep_time64.flags", &["TIMER_"]),
    ("dfd", &["AT_"]), ("olddfd", &["AT_"]), ("newdfd", &["AT_"]), ("flags", &["O_"]), ("prot", &["PROT_"]),
    ("which_clock", &["CLOCK_"]), ("op", &["FUTEX_"]),
];
/// Records of a variable length, one after another (a struct ending in a
/// flexible array: a directory's entries, getdents64's): the members that
/// say how long each record is, all of it.
pub const RECORD_LENGTHS: &[&str] = &["d_reclen"];
/// The errno names among the UAPI constants (errno-base.h's and errno.h's:
/// ENOENT, EEXIST): what a failed call's -errno is called.
pub const ERRNO: &str = "E";
