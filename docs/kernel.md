# The kernel: arch/wasm

Linux 7.2 compiled to WebAssembly, with nothing emulated: `linux/arch/wasm`
(submodule, branch `wasm`) is a minimal architecture port, and almost
everything else is the generic kernel. Outside arch/wasm the port changes
five files, 78 lines (build scripts, the initramfs's alignment, and nolibc's
`arch-wasm.h` for /init).

The configuration: wasm32, `CONFIG_MMU=n`, one CPU, `PREEMPT_NONE`, an
initramfs, a console (with input), a timer; the block layer, the loop
device and ext4.

## One Worker per task

Wasm cannot save and restore its own call stack, so a context switch cannot
be a stack swap. Instead **every task_struct gets its own Web Worker**
(kernel/process.c), and a parked Worker's native stack *is* the task's
saved context. All Workers instantiate the same kernel module on **one
shared `WebAssembly.Memory`**, so they share all of kernel memory; each has
its own wasm stack.

Exactly one Worker holds the CPU at a time. Each task has a token word
(`thread_info.cpu_token`); the others wait on theirs with
`memory.atomic.wait32`. `__switch_to(prev, next)` parks prev's token, gives
next the token, notifies it, and waits until its own comes back:

```text
Worker A (prev)                         Worker B (next)
__switch_to(A, B)                       ... waiting on B's token
  A.token = PARKED
  B.token = 0; notify(B.token)  ─────▶  wakes, returns from its
  wait(A.token)                         __switch_to (or starts)
  ...                                   runs
```

`copy_thread` gives the new task to the host (`spawn_task`): a Worker
calls the kernel's `task_entry(p)` export on the task's stack, which parks
until the task is first scheduled. A dead task's Worker unwinds its stack
(`exit_task`), and the host gives it another task later.

The boot Worker runs `start_kernel` and becomes the idle task (swapper).

## Interrupts and time

The host never interrupts running wasm code. It raises an interrupt by
setting a bit in `wasm_irq_pending` and notifying that word (kernel/irq.c).
The kernel takes pending interrupts when it re-enables interrupts, and the
idle loop waits on the word: that is the host's only way to wake the
kernel. For a `PREEMPT_NONE` kernel these are the points where real hardware
would deliver them too.

| Line | Raised by | For |
|---|---|---|
| 0, timer | the timer Worker | the clockevent (kernel/time.c) |
| 1, console | the page | console input (/init's doorbell, below) |

The clockevent is one-shot: the kernel writes its deadline to
`wasm_timer_deadline`; the host's timer Worker waits on that word
(`memory.atomic.wait64`, with a timeout until the deadline) and raises the
timer line when it passes. The clocksource reads host time.

## Programs, and /init

Programs are wasm modules with **their own memory** (so a program's
pointers are not kernel addresses; NOMMU without sharing the kernel's
memory). kernel/binfmt_wasm.c recognizes them in `exec`: the host compiles
and instantiates the module on the task's Worker (`user_load`), and enters
its `_start` when the task returns to user mode (`user_enter`). A syscall is
a plain wasm call into the kernel's `syscall` export, on the same Worker
(kernel/syscall.c). `copy_from_user`/`copy_to_user` are host imports that
copy between the two memories (`user_read`, `user_write`).

There is no user fork: a wasm program cannot copy its own stack.
`copy_thread` refuses one; kernel threads (which start from a function)
work as usual.

So there is one program: **/init** (user/init.c, C on nolibc). It mounts
the disk, and then makes the syscalls the page asks for, one at a time. It
waits for them reading its terminal: the page puts a request in shared
memory (the runtime's), and one byte of console input (`WASM_IRQ_CONSOLE`)
wakes it; the runtime's `ksee.call` import copies the request (the number,
the arguments, the buffers) into its memory, it makes the syscall, and
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

`CONFIG_WASM_HOST_MEMORY` (16 MiB) at the bottom of linear memory belongs to
the host runtime, which runs as wasm in the same memory; the kernel image is
linked above it. RAM ends where the host says (`memory_end`); the host puts
the initramfs at its top and passes `initrd=` on the command line. Memory
above RAM is the host's heap.

## The host interface

All of it is in arch/wasm/include/asm/wasm.h. Imports (module `kernel`):
`console_write`, `console_read`, `get_cmdline`, `memory_end`, `time_ns`, `spawn_task`,
`exit_task`, `halt`, `user_load`, `user_enter`, `user_read`, `user_write`,
`print_stack`; and `ksee.bulk` (see [recording.md](recording.md)).
Exports: `boot_stack`, `boot`, `task_entry`, `syscall`, `ksee_trace`, and the
globals `__stack_pointer`, `wasm_irq_pending`, `wasm_timer_deadline`,
`__current_task`.
