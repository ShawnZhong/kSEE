# The kernel: arch/wasm

Linux 7.2 compiled to WebAssembly, with nothing emulated: `linux/arch/wasm`
(submodule, branch `wasm`) is a minimal architecture port, and almost
everything else is the generic kernel. Outside arch/wasm the port changes
five files, 78 lines (build scripts, the initramfs's alignment, and nolibc's
`arch-wasm.h` for the programs).

The configuration: wasm32, `CONFIG_MMU=n`, one CPU, `PREEMPT_NONE`, an
initramfs, a console (output), a timer; the block layer, the loop
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
interrupt may be pending: the page's input, or (the clock being virtual,
below) the timer's deadline, skipped to at once. For a `PREEMPT_NONE`
kernel these are the points where real hardware would deliver them too.

| Line | Raised by | For |
|---|---|---|
| 0, timer | the host, when the deadline passes (at idle, or at a switch) | the clockevent (kernel/time.c) |
| 1, doorbell | the page | a task to ring: woken, its wait returns (the runners', below) |

The clockevent is one-shot: the kernel writes its deadline to
`wasm_timer_deadline`; the host raises the timer line once it passes. The
clocksource reads the host's clock, `time_ns`, which is **virtual**: it
moves on by a microsecond at each read (so delays, which spin on it, end),
and, while the page waits for the kernel (a boot, a call), idle skips it
to the timer's deadline at once; between the page's calls it stands still.
So a sleep takes no real time, and the machine is deterministic: the same
calls make the same run, record for record. The tick stops while the CPU
idles (`NO_HZ_IDLE`), so an idle CPU's next timer is a real one. While a
call is under way, idle skips to it only if the call's runner sleeps on a
timer of its own (the task a queued timer is to wake: an `hrtimer_sleeper`
among the hrtimers, a `process_timer` on the timer wheel; a sleep, a wait
with a timeout, however long; the runtime's sched.rs reads them by the
kernel's types, no kernel change); else the call is blocked:
it waits for another call, and the machine is quiet at once. (During the
boot, it skips a virtual second's worth: what the boot leaves for later.)

## Programs

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

## Threads, and vfork

A user `clone` cannot return twice: wasm has no way to copy a call stack.
So a clone child **starts over**, at its program's clone entry (its
`__wasm_clone_start` export; `user_start` gives it too), as a libc's
`clone(fn, stack, ...)` wrapper would call `fn` on the new stack. It must
share its parent's memory (`CLONE_VM`: without it, a copy at the same
addresses, which NOMMU cannot give; so no `fork`, as on any NOMMU
system):

- a **thread** (`CLONE_THREAD`, ...) runs on the stack it was given;
- **vfork** (`CLONE_VM | CLONE_VFORK`, no stack) runs below its parent's
  stack, while the parent waits (the kernel's own `CLONE_VFORK`) until the
  child execs or exits.

`copy_thread` tells the host (`clone_user`): a program's `__stack_pointer`
is its instance's global, which its threads share, so the host keeps each
task's value and puts it back when the task gets the CPU. After an
`execve` from user mode, the host drops the task's stack (the old
program's frames are never returned to) and starts it again on a fresh
one, in `user_entry` (`restart_task`). No `CLONE_SETTLS`: programs have no
thread-local storage. Futexes are the kernel's own
(`CONFIG_FUTEX`; one CPU, so plain loads and stores in user space).

## /init, the program, and the runners

There are two programs (C on nolibc). **/init** (user/init.c) sets up:
it mounts devtmpfs and the disk, then starts **/bin/prog** (user/prog.c)
as its vfork child, waits for it, and starts it again when it exits. The
program is the page's: a process as any (not pid 1), it makes the
syscalls the page asks for, one at a time.

The program waits for a call in a `futex` on the first word of its
request (`struct request`: the number, -1 while none is asked; the
arguments), in its own memory, which is kernel memory; `ksee.ready(&req,
heap, size, main)` tells the host where it is, and where its own memory
is (a static array: the page's variables, and each call's strings and
buffers, which the host puts there); `main`: it is the program itself, the
runner the page's calls go to unless they name another (`Machine.main`).
The page writes the call into the request (recorded, as any write to
kernel memory), the number last, and rings the runner's task
(`WASM_IRQ_DOORBELL`: its handler wakes the task, kernel/irq.c; the futex
wait, woken, sees the number changed and returns); it makes the syscall
between `ksee.begin` and `ksee.end(ret)`, which bracket the recording and
carry the result, and waits again. So every syscall the page shows is a
real process's, with its fds and its memory, and the protocol is a struct
in shared memory and a bracket. (A plain futex, not console input, a
signal, or a device: those would put a worker, a signal's delivery or the
VFS at the start of every call's recording; and no syscall of this port's
own.)

A clone child of the program (a thread, a child process: it shares the
program's memory, `CLONE_VM`) is a **runner**
too, and so is the program exec'd again (the doorbell rings one task). A
runner's `execve` call is over once the new program is loaded
(`user_start`): a runner again only if it says so (`ksee.ready`), as
/bin/prog does. So the page can make calls as several
threads and processes: one blocks (a futex's wait, a pipe's read),
another wakes it.

## A disk

No block device is emulated: the initramfs holds an ext4 image
(`/disk.img`, made by `mkfs.ext4` at build time), and /init attaches it to
the loop device (the kernel's own driver: a file as a block device) and
mounts it on `/mnt`. So ext4, jbd2 and the block layer are all the real
kernel's; the disk's blocks are that file's pages in memory. The image
comes with files for the examples to start from (`mkfs.ext4 -d`):
`/mnt/note.txt`, `/mnt/dir/a.txt`.

## Console

kernel/console.c: one console, printk's from `setup_arch` on: output only,
to the host, synchronously (`console_write`). `/dev/console`, the programs'
stdin, stdout and stderr, is a plain character device on it: a write goes
straight to the host, a read is at its end. There is no tty layer
(`CONFIG_TTY=n`): a tty's line discipline and locks would be in every
write's recording for nothing, and hvc's thread, kicked after every write,
added a task to every recording.

## Memory

The kernel's memory is the host runtime's (the kernel imports it; not
shared: one thread). `CONFIG_WASM_HOST_MEMORY` (16 MiB) at its bottom belongs
to the runtime, its static data and stack; the kernel image is linked above
it. RAM ends where the host says (`memory_end`); the host puts
the initramfs at its top and passes `initrd=` on the command line. Memory
above RAM is the host's heap.

## The host interface

All of it is in arch/wasm/include/asm/wasm.h. Imports (module `kernel`):
`console_write`, `get_cmdline`, `memory_end`, `time_ns`,
`spawn_task`, `switch_task`, `restart_task` and `idle` (suspending),
`clone_user`, `doorbell`, `halt`, `user_load`,
`user_start`, `print_stack`; and `ksee.bulk` (see [recording.md](recording.md)).
Exports: `boot_stack`, `boot`, `task_entry`, `user_entry`, `syscall`, `ksee_trace`, and the
globals `__stack_pointer`, `wasm_irq_pending`, `wasm_timer_deadline`,
`__current_task`.
