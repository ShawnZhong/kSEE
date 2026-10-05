# The kernel: arch/wasm

Linux stable `v7.2.7` is compiled directly to WebAssembly, with nothing
emulated. `arch/wasm` is a minimal architecture port, the repository's `arch/wasm/`;
the build fetches that stable release into `linux/`, applies `linux.patch`
and copies `arch/wasm/` in. Almost everything else is generic kernel code:
`linux.patch` holds only the build and portability changes the port needs
in Linux's own files.

The configuration: wasm32, no MMU, one CPU, an initramfs, a console
(output), a timer, a disk; the block layer and ext4.

## Tasks: stacks the host suspends

Wasm cannot save and restore its own call stack, so a context switch
cannot be a stack swap. Instead the kernel is **one wasm instance, on one
thread, and a task's context is a call stack of it**, which the host
suspends and resumes with JSPI (JavaScript Promise Integration: a wasm
stack suspends on a promise, and resumes when it settles).

`__switch_to(prev, next)` calls the host's `switch_task(prev, next,
dead)`, a *suspending* import: it returns a promise, so prev's stack
suspends; the host then gives the CPU to next: resumes next's stack (or,
the first time, starts it: `task_entry(p)`, on the stack `copy_thread`
gave with `spawn_task`). A dead task's stack is never resumed (the host
drops it). All tasks share the instance's `__stack_pointer` (the C
stack's, in memory): the host saves it when a task suspends, and puts it
back before it resumes.

```text
task A (prev)                           host                    task B (next)
__switch_to(A, B)
  switch_task(A, B, dead) ──promise──▶ A suspends
                                        B's promise resolved ──▶ returns from its
                                                                 switch_task (or starts)
```

The boot CPU runs `start_kernel` (the host calls `boot`, on `boot_stack`)
and becomes the idle task (swapper). Only one task runs at a time: one
CPU, and no host code runs while the kernel does.

## Interrupts and time

The host never interrupts running wasm code: its JavaScript runs only
while the kernel is suspended. It raises an interrupt by setting a bit in
`wasm_irq_pending`; the kernel takes pending interrupts when it re-enables
interrupts. The idle loop suspends until an interrupt may be pending
(`idle`, suspending): the page's input, or (the clock being virtual,
below) the timer's deadline, skipped to at once.

| Line | Raised by | For |
|---|---|---|
| 0, timer | the host, when the deadline passes (at idle, or at a switch) | the clockevent (kernel/time.c) |
| 1, doorbell | the page | the next task with something for it (`doorbell`, irq.c): woken, its wait returns |

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
timer of its own (an `hrtimer_sleeper` among the hrtimers, a
`process_timer` on the timer wheel; a sleep, a wait with a timeout,
however long; `ksee_core::arch::sleeping` reads them by the kernel's
types, no kernel change); else the call is blocked: it waits for another
call, and the machine is quiet at once.

## Programs

Programs are **position-independent** wasm modules (`-fPIC -pie`) that
import the kernel's memory: as on any NOMMU system, a program lives in
pages the kernel gives it, and its pointers are kernel addresses.
kernel/binfmt_wasm.c recognizes them in `exec`: the host compiles the
module (`user_load`: the bytes its data needs), the kernel maps it pages
for its data and a 64 KiB stack, and the host instantiates it there
(`user_start`: its `_start`, as an index in the kernel's function table,
and where its clone children start, its `__wasm_clone_start` export),
which the task calls when it returns to user mode. A syscall is a plain
wasm call into the kernel's `syscall` export (kernel/syscall.c). Its
`sys_call_table` entries are weak: one this kernel does not build has
none, NULL, and returns -ENOSYS; so the table says which syscalls there
are, and the build's pack reads them from it (the page offers only those).

So `copy_from_user`/`copy_to_user` are plain copies, recorded like any
other, and a program's buffers are in the recording's memory. `access_ok`
checks that a user pointer is in the RAM the kernel allocates from: below
it are the kernel image and the runtime's own memory. There is no
protection otherwise (NOMMU): a program can write the kernel's memory.

## Threads, and vfork

A user `clone` cannot return twice: wasm has no way to copy a call stack.
So a clone child **starts over**, at its program's clone entry, as a
libc's `clone(fn, stack, ...)` wrapper would call `fn` on the new stack.
It must share its parent's memory (`CLONE_VM`: without it, a copy at the
same addresses, which NOMMU cannot give; so no `fork`, as on any NOMMU
system):

- a **thread** runs on the stack it was given;
- **vfork** (no stack) runs below its parent's stack, while the parent
  waits until the child execs or exits.

`copy_thread` tells the host (`clone_user`): a program's `__stack_pointer`
is its instance's global, which its threads share, so the host keeps each
task's value and puts it back when the task gets the CPU. After an
`execve` from user mode, the host drops the task's stack (the old
program's frames are never returned to) and starts it again on a fresh
one, in `user_entry`. No `CLONE_SETTLS`: programs have no thread-local
storage. Futexes are the kernel's own (one CPU, so plain loads and stores
in user space).

## /init, the program, and the runners

There are two programs (plain C, no C library: user/syscall.h makes their
syscalls, a call to the kernel's syscall export). **/init** (user/init.c)
sets up: it mounts the disk (below), then starts **/bin/prog**
(user/prog.c) as its vfork child, waits for it, and starts it again when
it exits. The program is the page's: a process as any (not pid 1), it
makes the syscalls the page asks for, one at a time.

The program waits for a call in a `futex` on the first word of its
request (`struct request`: the number, -1 while none is asked; the
arguments), in its own memory, which is kernel memory; `ksee.ready(&req,
heap, size, main)` tells the host where it is, and where its own memory
is (a static array: the page's variables, and each call's strings and
compound literals, which the host puts there); `main`: it is the program
itself, the runner the page's calls go to unless they name another. The
page writes the call into the request (recorded, as any write to kernel
memory), the number last, and rings the runner's task (the doorbell: its
futex wait, woken, sees the number changed and returns); it makes the
syscall between `ksee.begin` and `ksee.end(ret)`, which bracket the
recording and carry the result, and waits again. So every syscall the page
shows is a real process's, with its fds and its memory, and the protocol
is a struct in shared memory and a bracket. (A plain futex, not console
input, a signal, or a device: those would put a worker, a signal's
delivery or the VFS at the start of every call's recording; and no syscall
of this port's own.)

A clone child of the program (a thread, a child process: it shares the
program's memory) is a **runner** too, and so is the program exec'd again
(the doorbell rings one task). A runner's `execve` call is over once the
new program is loaded: a runner again only if it says so
(`ksee.ready`), as /bin/prog does. So the page can make calls as several
threads and processes: one blocks (a futex's wait, a pipe's read), another
wakes it.

## A disk

The disk, `/dev/wda`, is the host's: its blocks are an ext4 image
(`disk.img`, made by `mkfs.ext4` at build time; each boot starts from it),
and drivers/disk.c drives it as a disk controller is driven. Its blk-mq
`queue_rq` hands the host a request's segments with the request's tag (as a
controller is handed commands), and returns: the task that wants the blocks
sleeps (another runs, or the CPU idles). The host makes the transfers when
the CPU next idles (a read: written into kernel memory, logged, as the
host's writes are), posts the tags of the requests done, and raises
`WASM_IRQ_DISK`; the interrupt maps each tag to its request
(`blk_mq_tag_to_rq`) and ends it, which wakes its waiter. /init mounts it on `/mnt`. So
ext4, jbd2, the block layer and the request queue are all the real
kernel's. The image
comes with files for the lessons to start from (`mkfs.ext4 -d user/disk`):
`/mnt/note.txt`, `/mnt/dir/a.txt`.

## Console

drivers/console.c: one console, printk's from `setup_arch` on: output only,
to the host, synchronously (`console_write`). `/dev/console`, the
programs' stdin, stdout and stderr, is a plain character device on it: a
write goes straight to the host, a read is at its end. There is no tty
layer (`CONFIG_TTY=n`): a tty's line discipline and locks would be in
every write's recording for nothing.

## Memory

The kernel's memory is the host runtime's (the kernel imports it; not
shared: one thread). `CONFIG_WASM_HOST_MEMORY` (16 MiB) at its bottom
belongs to the runtime, its static data and stack; the kernel image is
linked above it. RAM ends where the host says (`memory_end`: 64 MiB, see
ksee-host's state.rs); the host puts the initramfs at its top and passes `initrd=`
on the command line. Memory above RAM is the host's heap.

## The host interface

All of it is in arch/wasm/include/asm/wasm.h (and the Makefile's
`--export`s):

| | The kernel imports (module `kernel`) | The kernel exports |
|---|---|---|
| Boot and memory | `get_cmdline`, `memory_end` | `boot_stack`, `boot` |
| Time, the console | `time_ns`, `console_write`, `print_stack`, `halt` | |
| Tasks (JSPI: `switch_task`, `idle`, `restart_task` suspend) | `spawn_task`, `clone_user`, `switch_task`, `restart_task`, `idle`, `doorbell` | `task_entry`, `user_entry`, `__stack_pointer`, `__current_task` |
| Programs | `user_load`, `user_start` | `syscall` |
| Interrupts, the disk | `disk_sectors`, `disk_submit`, `disk_done` | `wasm_irq_pending`, `wasm_timer_deadline` |
| The trace ([recording.md](recording.md)) | | `ksee_trace` (the ring), `ksee_bulk`, `ksee_record` |
