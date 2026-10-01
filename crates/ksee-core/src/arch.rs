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
/// The syscall table (arch/wasm/kernel/syscall.c): an entry per number,
/// the function it calls (SYSCALL_WRAPPER + its name), NULL for a
/// syscall this kernel does not build (the linker found none): ksee pack
/// reads the syscalls from it.
pub const SYSCALL_TABLE: &str = "sys_call_table";
/// The machine's own primitives (arch/wasm/include/asm and the generic
/// half of the arch layer: `current`, the interrupt flag, atomics, bit
/// ops), inlined all over the kernel: the machine, not calls. Their code
/// counts as the line it is inlined at.
pub(crate) const PRIMITIVES: [&str; 2] = ["arch/wasm/include/asm/", "include/asm-generic/"];
/// The machine's own variables (arch/wasm: the interrupt flag, the task
/// running, the timer), written by every lock and switch: the machine's
/// state, not the kernel's.
pub const MACHINE: &str = "arch/wasm/";
/// Where a task starts (arch/wasm/kernel/process.c), with an empty stack.
pub(crate) const TASK_ENTRY: &str = "wasm_task_entry";
/// The context switch: switch(prev, next) returns on next's stack.
pub(crate) const SWITCH: &str = "__switch_to";
/// The kernel's page (arch/wasm: PAGE_SHIFT 12).
pub const PAGE: u32 = 4096;
/// A task (what the task running points at), and the fields of it the
/// Stack shows first.
pub const TASK: &str = "task_struct";
/// What the kernel calls the task running, and a task's id and name.
pub(crate) const CURRENT: &str = "current";
pub(crate) const TASK_PID: &str = "pid";
pub(crate) const TASK_COMM: &str = "comm";
/// A task's parent, and its thread group's leader (itself, for a process).
pub(crate) const TASK_PARENT: &str = "real_parent";
pub(crate) const TASK_LEADER: &str = "group_leader";
/// The kernel's tasks, as for_each_process_thread walks them: from
/// init_task, each process by its `tasks` link, and each process's
/// threads, its signal_struct's `thread_head` list, by their `thread_node`.
pub(crate) const INIT_TASK: &str = "init_task";
pub(crate) const TASK_LIST: &str = "tasks";
pub(crate) const TASK_SIGNAL: (&str, &str) = ("signal", "signal_struct");
pub(crate) const THREAD_HEAD: &str = "thread_head";
pub(crate) const THREAD_NODE: &str = "thread_node";
/// Whether a task is on its CPU's run queue (runnable, or running).
pub(crate) const TASK_ON_RQ: &str = "on_rq";
/// A task's fields shown first (the Tasks'), in this order.
pub(crate) const TASK_FIELDS: [&str; 12] = ["pid", "tgid", "comm", "__state", "flags", "prio", "mm", "files", "fs", "real_parent", "group_leader", "signal"];
/// A syscall's body (SYSCALL_DEFINE's `__do_sys_NAME`), whose parameters
/// are the syscall's.
pub(crate) const SYSCALL_BODY: &str = "__do_sys_";
/// The constants that fit a syscall's parameter, by the starts of their
/// names: for "syscall.param", else any syscall's parameter of that name.
pub(crate) const SYSCALL_CONSTS: &[(&str, &[&str])] = &[
    ("mmap2.flags", &["MAP_"]), ("unlinkat.flag", &["AT_"]), ("renameat2.flags", &["RENAME_"]), ("linkat.flags", &["AT_"]),
    ("io_uring_enter.flags", &["IORING_ENTER_"]),
    ("prctl.option", &["PR_"]), ("waitid.which", &["P_"]), ("waitid.options", &["W"]), ("clone.clone_flags", &["CLONE_", "SIG"]),
    ("clock_nanosleep.flags", &["TIMER_"]),
    ("dfd", &["AT_"]), ("olddfd", &["AT_"]), ("newdfd", &["AT_"]), ("flags", &["O_"]), ("prot", &["PROT_"]),
    ("which_clock", &["CLOCK_"]), ("op", &["FUTEX_"]),
];
/// The starts of the names of the constants that fit syscall `syscall`'s
/// parameter `param` (SYSCALL_CONSTS).
pub(crate) fn consts_of(syscall: &str, param: &str) -> &'static [&'static str] {
    let key = alloc::format!("{syscall}.{param}");
    let consts = SYSCALL_CONSTS;
    consts.iter().find(|(k, _)| *k == key).or_else(|| consts.iter().find(|(k, _)| *k == param)).map_or(&[], |(_, c)| c)
}

/// Records of a variable length, one after another (a struct ending in a
/// flexible array: a directory's entries, getdents64's): the members that
/// say how long each record is, all of it.
pub(crate) const RECORD_LENGTHS: &[&str] = &["d_reclen"];
/// The errno names among the UAPI constants (errno-base.h's and errno.h's:
/// ENOENT, EEXIST): what a failed call's -errno is called.
pub(crate) const ERRNO: &str = "E";
/// The largest errno (include/linux/err.h's MAX_ERRNO): a result from -1
/// down to -MAX_ERRNO is an error.
pub(crate) const MAX_ERRNO: i64 = 4095;

/// The types that hold flags, printed in hex (`*`: a name's end).
pub(crate) const FLAG_TYPES: &[&str] = &["fmode_t", "gfp_t", "*flags_t"];
/// The types printed in octal: file modes.
pub(crate) const OCTAL_TYPES: &[&str] = &["umode_t"];

/// The variables the kernel's macros make up (`__UNIQUE_ID`: min_t's,
/// max_t's temporaries), not the source's own: not shown.
pub(crate) const MADE_UP: &str = "__UNIQUE_ID_";

/// The kernel's areas, by where a function's file is (the first that fits;
/// the timeline colors a call by its area's, the last gray): its
/// subsystems' directories.
pub const AREAS: &[(&str, &str)] = &[("fs/", "fs"), ("mm/", "mm"), ("kernel/sched/", "sched"), ("kernel/", "kernel"),
                                     ("drivers/", "drivers"), ("lib/", "lib"), ("include/", "include"), ("arch/", "arch")];

/// The color of a call in file `file` (CSS): a hue for each of AREAS in
/// order, the last (arch), and none, gray; `on` (at the cursor), darker.
pub fn color(file: Option<&str>, on: bool) -> alloc::string::String {
    const HUES: [i32; 7] = [210, 135, 28, 48, 280, 175, 330];
    let file = file.unwrap_or("");
    let l = if on { 50 } else { 74 };
    match AREAS.iter().position(|(p, _)| file.starts_with(p)).and_then(|k| HUES.get(k)) {
        Some(h) => alloc::format!("hsl({h} {}% {l}%)", if on { 65 } else { 55 }),
        None => alloc::format!("hsl(0 0% {}%)", l + 5),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_calls_color_is_its_area() {
        let (fs, arch) = (super::AREAS[0].0, super::AREAS.last().unwrap().0);
        assert_eq!(super::color(Some(&alloc::format!("{fs}x.c")), false), "hsl(210 55% 74%)");
        assert_eq!(super::color(Some(&alloc::format!("{fs}x.c")), true), "hsl(210 65% 50%)");
        // (The last, arch, and none: gray.)
        assert_eq!(super::color(Some(&alloc::format!("{arch}x.c")), false), "hsl(0 0% 79%)");
        assert_eq!(super::color(None, false), "hsl(0 0% 79%)");
    }
}
