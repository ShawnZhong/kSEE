# The kernel: arch/wasm

Linux 7.2 compiled to WebAssembly, with nothing emulated: `linux/arch/wasm`
(submodule, branch `wasm`) is a minimal architecture port, and almost
everything else is the generic kernel. Outside arch/wasm the port changes
five files, 78 lines (build scripts, the initramfs's alignment, and nolibc's
`arch-wasm.h` for /init).

The configuration: wasm32, `CONFIG_MMU=n`, one CPU, `PREEMPT_NONE`, an
initramfs, a console (with input), a timer; the block layer, the loop
device and ext4.

## Tasks: stacks the host suspends

Wasm cannot save and restore its own call stack, so a context switch
cannot be a stack swap. Instead the kernel is **one wasm instance, on one
thread, and a task's context is a call stack of it**, which the host
suspends and resumes with JSPI (JavaScript Promise Integration: a wasm
stack suspends on a promise, and resumes when it settles).

`__switch_to(prev, next)` calls the host's `switch_task(prev, next)`, a
*suspending* import: it returns a promise, so prev's stack suspends; the
host then gives the CPU to next: resolves next's promise (its stack
resumes, returning from its own `__switch_to`), or, the first time, starts
it (`task_entry(p)`, on the stack `copy_thread` gave with `spawn_task`).
prev resumes when some task switches back to it; a dead task's stack is
never resumed (the host drops it). All tasks share the instance's
`__stack_pointer` (the C stack's, in memory): the host saves it when a task
suspends, and puts it back before it resumes.

```text
task A (prev)                           host                    task B (next)
__switch_to(A, B)
  switch_task(A, B) ──promise──▶ A suspends
                                        B's promise resolved ──▶ returns from its
                                                                 switch_task (or starts)
```

The boot CPU runs `start_kernel` (the host calls `boot`) and becomes the
idle task (swapper). Only one task runs at a time: one CPU, and no host
code runs while the kernel does.

## Interrupts and time

The host never interrupts running wasm code: its JavaScript runs only
while the kernel is suspended. It raises an interrupt by setting a bit in
`wasm_irq_pending`; the kernel takes pending interrupts when it re-enables
interrupts. The idle loop calls `idle()`, which suspends until an
interrupt may be pending: the timer's deadline passed (a `setTimeout`), or
input from the page. For a `PREEMPT_NONE` kernel these are the points
where real hardware would deliver them too.

| Line | Raised by | For |
|---|---|---|
| 0, timer | the host, when the deadline passes (at idle, or at a switch) | the clockevent (kernel/time.c) |
| 1, console | the page | console input (/init's doorbell, below) |

The clockevent is one-shot: the kernel writes its deadline to
`wasm_timer_deadline`; the host raises the timer line once it passes. The
clocksource reads the host's clock, `time_ns`, which is **virtual**: it
moves on by a microsecond at each read (so delays, which spin on it, end),
and, while the page waits for the kernel (a boot, a call), idle skips it
to the timer's deadline at once; between the page's calls it stands still.
So a sleep takes no real time, and the machine is deterministic: the same
calls make the same run, record for record.

## Programs, and /init

Programs are **position-independent** wasm modules (`-fPIC -pie`, the
tool conventions' dynamic linking) that import the kernel's memory: as on
any NOMMU system, a program lives in pages the kernel gives it, and its
pointers are kernel addresses. kernel/binfmt_wasm.c recognizes them in
`exec`: the host compiles the module (`user_load`: how much data it
needs), the kernel maps it pages for its data and a 64 KiB stack
(`vm_mmap`), and the host instantiates it there (`user_start`: its
`__memory_base` and `__stack_pointer`) and puts its `_start` in the
kernel's function table, which the task calls when it returns to user mode
(a direct call: nothing of the host's may be on a stack it suspends). A
syscall is a plain wasm call into the kernel's `syscall` export
(kernel/syscall.c).

So `copy_from_user`/`copy_to_user` are plain copies (`UACCESS_MEMCPY`),
recorded like any other, and a program's buffers are in the recording's
memory. `access_ok` checks that a user pointer is in the RAM the kernel
allocates from: below it are the kernel image and the runtime's own memory.
There is no protection otherwise (NOMMU): a program can write the kernel's
memory.

There is no user fork: a wasm program cannot copy its own stack.
`copy_thread` refuses one; kernel threads (which start from a function)
work as usual.

So there is one program: **/init** (user/init.c, C on nolibc). It mounts
the disk, and then makes the syscalls the page asks for, one at a time. It
waits for them reading its terminal: the page leaves a request with the
runtime, and one byte of console input (`WASM_IRQ_CONSOLE`) wakes it; the
runtime's `ksee.call` import copies the request (the number, the
arguments, the buffers) into its data (recorded), it makes the syscall, and
`ksee.done` gives the result back. So every syscall the page shows is a
real process's, with its fds and its memory.

## A disk

No block device is emulated: the initramfs holds an ext4 image
(`/disk.img`, made by `mkfs.ext4` at build time), and /init attaches it to
the loop device (the kernel's own driver: a file as a block device) and
mounts it on `/mnt`. So ext4, jbd2 and the block layer are all the real
kernel's; the disk's blocks are that file's pages in memory.

## Console

kernel/console.c: one console, printk's from `setup_arch` on, whose tty is
`/dev/console`. Writes go to the host synchronously (`console_write`);
input comes with `WASM_IRQ_CONSOLE` (`console_read`). (A console of its own rather than hvc: hvc's thread,
kicked after every write, added a task and noise to every recording.)

## Memory

The kernel's memory is the host runtime's (the kernel imports it; not
shared: one thread). `CONFIG_WASM_HOST_MEMORY` (16 MiB) at its bottom belongs
to the runtime, its static data and stack; the kernel image is linked above
it. RAM ends where the host says (`memory_end`); the host puts
the initramfs at its top and passes `initrd=` on the command line. Memory
above RAM is the host's heap.

## The host interface

All of it is in arch/wasm/include/asm/wasm.h. Imports (module `kernel`):
`console_write`, `console_read`, `get_cmdline`, `memory_end`, `time_ns`,
`spawn_task`, `switch_task` and `idle` (suspending), `halt`, `user_load`,
`user_start`, `print_stack`; and `ksee.bulk` (see [recording.md](recording.md)).
Exports: `boot_stack`, `boot`, `task_entry`, `syscall`, `ksee_trace`, and the
globals `__stack_pointer`, `wasm_irq_pending`, `wasm_timer_deadline`,
`__current_task`.
