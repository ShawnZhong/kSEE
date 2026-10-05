//! The kernel's names the host rebuilds and shows a recording by (arch/wasm's
//! functions, the scheduler's, and a few of its types and conventions), and
//! runs it by (its timers): the only kernel names ksee-core knows.

use crate::types::Types;
use crate::Memory;

/// A syscall (arch/wasm/kernel/syscall.c): its call's arguments are the
/// number and the syscall's; its return's value is the result.
pub const SYSCALL: &str = "wasm_syscall";
/// A syscall's table entry (arch/wasm/include/asm/syscall_wrapper.h:
/// `__wasm_sys_read`), the one function SYSCALL_DEFINE makes for it: its
/// source's parameters (the SYSCALL_DEFINE line's) are the syscall's
/// arguments, on the entry's record.
/// (crates/ksee-cli's pack names the syscalls by it: the table's entries.)
pub const SYSCALL_WRAPPER: &str = "__wasm_sys_";
/// The machine's own primitives (arch/wasm/include/asm and the generic
/// half of the arch layer: `current`, the interrupt flag, atomics, bit
/// ops), inlined all over the kernel: the machine, not calls. Their code
/// counts as the line it is inlined at.
pub(crate) const PRIMITIVES: [&str; 2] = ["arch/wasm/include/asm/", "include/asm-generic/"];
/// The machine's own variables (arch/wasm: the interrupt flag, the task
/// running, the timer), written by every lock and switch: the machine's
/// state, not the kernel's.
pub(crate) const MACHINE: &str = "arch/wasm/";
/// Where a task starts (arch/wasm/kernel/process.c), with an empty stack.
pub(crate) const TASK_ENTRY: &str = "wasm_task_entry";
/// The context switch: switch(prev, next) returns on next's stack.
pub(crate) const SWITCH: &str = "__switch_to";
/// A task (what the task running points at), and the fields of it the
/// Stack shows first.
pub(crate) const TASK: &str = "task_struct";
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

/// The kernel's areas, by where their sources are: the timeline colors a
/// call by the first its file is in (its legend, in this order: by concept,
/// files and storage, memory, tasks, the machine (its devices, the port),
/// then the helpers).
pub const AREAS: &[(&str, &str)] = &[
    ("fs/", "fs"),
    ("block/", "block"),
    ("mm/", "mm"),
    ("kernel/sched/", "sched"),
    ("kernel/", "kernel"),
    ("drivers/", "drivers"),
    ("arch/", "arch"),
    ("lib/", "lib"),
    ("include/", "include"),
];

/// Whether one of `tasks` sleeps on a timer of its own (a sleep, a wait
/// with a timeout), as `mem` is now: the host then skips to the timer's
/// deadline. The kernel's timers, read as its types lay them out:
/// - hrtimers (an hrtimer_sleeper's task): each clock base's, in order
///   (7.2's linked rbtree: from the leftmost, by next);
/// - the timer wheel (a process_timer's task: schedule_timeout, msleep):
///   each base's buckets' lists.
pub fn sleeping(t: &Types, mem: &dyn Memory, tasks: &[u32]) -> bool {
    let off = |s: &str, f: &str| t.field(s, f).map(|(off, _)| off);
    let size = |s: &str| t.find(s).map_or(0, |id| t.size_of(id));
    // (How far a timer list is walked: kernel memory read live may be torn.)
    const MAX_WALK: u32 = 4096;
    // (A list's nodes from `first`, by the next pointer at `next`.)
    let wakes = |mut p: u32, next: u32, task: u32| {
        for _ in 0..MAX_WALK {
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

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn a_task_on_the_timer_wheel_sleeps() {
        // (One base of two buckets; a process_timer, its timer_list's node
        // first, its task after it, in the second bucket.)
        let t = Types::parse("T\t1\tbase\tint\t4\tu\n\
            T\t2\tstruct\thlist_head\t4\nM\t2\tfirst\t0\t0\t1\n\
            T\t3\tstruct\thlist_node\t8\nM\t3\tnext\t0\t0\t1\nM\t3\tpprev\t32\t0\t1\n\
            T\t4\tstruct\t\t8\n\
            T\t5\tstruct\ttimer_base\t8\nM\t5\tvectors\t0\t0\t4\n\
            T\t6\tstruct\ttimer_list\t12\nM\t6\tentry\t0\t0\t3\n\
            T\t7\tstruct\tprocess_timer\t16\nM\t7\ttimer\t0\t0\t6\nM\t7\ttask\t96\t0\t1\n\
            G\ttimer_bases\t256\t5\n");
        let mut m = alloc::vec![0u8; 1024];
        m[260..264].copy_from_slice(&512u32.to_le_bytes());
        m[524..528].copy_from_slice(&0x777u32.to_le_bytes());
        let mem = m;
        assert!(sleeping(&t, &mem, &[0x777]));
        assert!(!sleeping(&t, &mem, &[0x778]));
    }
}
