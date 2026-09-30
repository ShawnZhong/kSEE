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
pub const PRIMITIVES: [&str; 2] = ["arch/wasm/include/asm/", "include/asm-generic/"];
/// The machine's own variables (arch/wasm: the interrupt flag, the task
/// running, the timer), written by every lock and switch: the machine's
/// state, not the kernel's.
pub const MACHINE: &str = "arch/wasm/";
/// Where a task starts (arch/wasm/kernel/process.c), with an empty stack.
pub const TASK_ENTRY: &str = "wasm_task_entry";
/// The context switch: switch(prev, next) returns on next's stack.
pub const SWITCH: &str = "__switch_to";
/// The kernel's page (arch/wasm: PAGE_SHIFT 12).
pub const PAGE: u32 = 4096;
/// A task (what the task running points at), and the fields of it the
/// Stack shows first.
pub const TASK: &str = "task_struct";
/// What the kernel calls the task running, and a task's id and name.
pub const CURRENT: &str = "current";
pub const TASK_PID: &str = "pid";
pub const TASK_COMM: &str = "comm";
/// A task's parent, and its thread group's leader (itself, for a process).
pub const TASK_PARENT: &str = "real_parent";
pub const TASK_LEADER: &str = "group_leader";
/// The kernel's tasks, as for_each_process_thread walks them: from
/// init_task, each process by its `tasks` link, and each process's
/// threads, its signal_struct's `thread_head` list, by their `thread_node`.
pub const INIT_TASK: &str = "init_task";
pub const TASK_LIST: &str = "tasks";
pub const TASK_SIGNAL: (&str, &str) = ("signal", "signal_struct");
pub const THREAD_HEAD: &str = "thread_head";
pub const THREAD_NODE: &str = "thread_node";
/// Whether a task is on its CPU's run queue (runnable, or running).
pub const TASK_ON_RQ: &str = "on_rq";
/// A task's fields shown first (the Tasks'), in this order.
pub const TASK_FIELDS: [&str; 12] = ["pid", "tgid", "comm", "__state", "flags", "prio", "mm", "files", "fs", "real_parent", "group_leader", "signal"];
/// A syscall's body (SYSCALL_DEFINE's `__do_sys_NAME`), whose parameters
/// are the syscall's.
pub const SYSCALL_BODY: &str = "__do_sys_";
/// The constants that fit a syscall's parameter, by the starts of their
/// names: for "syscall.param", else any syscall's parameter of that name.
pub const SYSCALL_CONSTS: &[(&str, &[&str])] = &[
    ("mmap2.flags", &["MAP_"]), ("unlinkat.flag", &["AT_"]), ("renameat2.flags", &["RENAME_"]),
    ("prctl.option", &["PR_"]), ("waitid.which", &["P_"]), ("waitid.options", &["W"]), ("clone.clone_flags", &["CLONE_", "SIG"]),
    ("clock_nanosleep.flags", &["TIMER_"]),
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
/// The largest errno (include/linux/err.h's MAX_ERRNO): a result from -1
/// down to -MAX_ERRNO is an error.
pub const MAX_ERRNO: i64 = 4095;

/// The types that hold flags, printed in hex (`*`: a name's end).
pub const FLAG_TYPES: &[&str] = &["fmode_t", "gfp_t", "*flags_t"];
/// The types printed in octal: file modes.
pub const OCTAL_TYPES: &[&str] = &["umode_t"];

/// Whether one of `tasks` is the one a queued timer is to wake (it sleeps
/// on a timer: a sleep, a wait with a timeout). The kernel's own timers,
/// read as its types lay them out:
/// - hrtimers (an hrtimer_sleeper's task): each clock base's, in order
///   (7.2's linked rbtree: from the leftmost, by next);
/// - the timer wheel (a process_timer's task: schedule_timeout, msleep):
///   each base's buckets' lists.
pub fn timer_wakes(t: &crate::types::Types, mem: &dyn crate::Memory, tasks: &[u32]) -> bool {
    let off = |s: &str, f: &str| t.field(s, f).map(|(off, _)| off);
    let size = |s: &str| t.find(s).map_or(0, |id| t.size_of(id));
    // (A list's nodes from `first`, by the next pointer at `next`.)
    let wakes = |mut p: u32, next: u32, task: u32| {
        for _ in 0..4096 {
            if p == 0 {
                return false;
            }
            if tasks.contains(&mem.read_u32(p + task)) {
                return true;
            }
            p = mem.read_u32(p + next);
        }
        false
    };
    // (A timer's list node is at its start: rb_node_linked in the hrtimer's
    // timerqueue_linked_node; hlist_node in the timer_list.)
    let hr = || -> Option<bool> {
        let &(bases, _) = t.globals.get("hrtimer_bases")?;
        let (clock, all) = t.field("hrtimer_cpu_base", "clock_base")?;
        let (active, leftmost, next, task) = (off("hrtimer_clock_base", "active")?, off("rb_root_linked", "rb_leftmost")?,
                                              off("rb_node_linked", "next")?, off("hrtimer_sleeper", "task")?);
        let each = size("hrtimer_clock_base");
        Some((0..all.checked_div(each)?).any(|b| wakes(mem.read_u32(bases + clock + b * each + active + leftmost), next, task)))
    };
    let wheel = || -> Option<bool> {
        let &(bases, ty) = t.globals.get("timer_bases")?;
        let (vectors, all) = t.field("timer_base", "vectors")?;
        let (first, next) = (off("hlist_head", "first")?, off("hlist_node", "next")?);
        let task = off("process_timer", "task").unwrap_or(size("timer_list"));
        let (each, heads) = (size("timer_base"), all / 4);
        Some((0..t.size_of(ty).checked_div(each)?).any(|b| {
            (0..heads).any(|h| wakes(mem.read_u32(bases + b * each + vectors + h * 4 + first), next, task))
        }))
    };
    hr() == Some(true) || wheel() == Some(true)
}

/// The variables the kernel's macros make up (`__UNIQUE_ID`: min_t's,
/// max_t's temporaries), not the source's own: not shown.
pub const MADE_UP: &str = "__UNIQUE_ID_";

/// The kernel's areas, by where a function's file is (the first that fits;
/// the timeline colors a call by its area's, the last gray): its
/// subsystems' directories.
pub const AREAS: &[(&str, &str)] = &[("fs/", "fs"), ("mm/", "mm"), ("kernel/sched/", "sched"), ("kernel/", "kernel"),
                                     ("drivers/", "drivers"), ("lib/", "lib"), ("include/", "include"), ("arch/", "arch")];
