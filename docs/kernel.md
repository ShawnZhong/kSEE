# The kernel: arch/wasm

Linux 7.2 compiled to WebAssembly, with nothing emulated: `linux/arch/wasm`
(submodule, branch `wasm`) is a minimal architecture port, and almost
everything else is the generic kernel. Outside arch/wasm the port changes
five files, 78 lines (build scripts, the initramfs's alignment, and nolibc's
`arch-wasm.h` for the programs).

The configuration: wasm32, `CONFIG_MMU=n`, one CPU, `PREEMPT_NONE`, an
initramfs, a console, a timer.

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

`copy_thread` asks the host for a new Worker (`spawn_task`), which calls
the kernel's `task_entry(p)` export on the task's stack; it parks until the
task is first scheduled. A dead task's Worker unwinds itself (`exit_task`).

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
| 1, run | the page | running a program (below) |

The clockevent is one-shot: the kernel writes its deadline to
`wasm_timer_deadline`; the host's timer Worker waits on that word
(`memory.atomic.wait64`, with a timeout until the deadline) and raises the
timer line when it passes. The clocksource reads host time.

## Programs

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

So the kernel starts the page's programs itself (kernel/run.c), the way it
starts init: as a usermode helper. The request is the exported `wasm_run`:

```c
struct { u32 state; s32 status; s32 pid; char path[256]; } wasm_run;
```

The page writes a path, sets `REQUESTED` and raises the run line; the
interrupt queues work that runs the program with `/dev/console` as its
fds 0–2, waits for it, and writes its exit status and pid (`DONE`); the page
takes them and sets `IDLE`. `/init` (user/init.c) only has to exist.

## Console

kernel/console.c: one console, printk's from `setup_arch` on, whose tty is
`/dev/console`. Writes go to the host synchronously (`console_write`); the
host sends no input. (A console of its own rather than hvc: hvc's thread,
kicked after every write, added a task and noise to every recording.)

## Memory

`CONFIG_WASM_HOST_MEMORY` (16 MiB) at the bottom of linear memory belongs to
the host runtime, which runs as wasm in the same memory; the kernel image is
linked above it. RAM ends where the host says (`memory_end`); the host puts
the initramfs at its top and passes `initrd=` on the command line. Memory
above RAM is the host's heap.

## The host interface

All of it is in arch/wasm/include/asm/wasm.h. Imports (module `kernel`):
`console_write`, `get_cmdline`, `memory_end`, `time_ns`, `spawn_task`,
`exit_task`, `halt`, `user_load`, `user_enter`, `user_read`, `user_write`,
`print_stack`; and `ksee.bulk` (see [recording.md](recording.md)).
Exports: `boot_stack`, `boot`, `task_entry`, `syscall`, `ksee_trace`, and the
globals `__stack_pointer`, `wasm_irq_pending`, `wasm_timer_deadline`,
`__current_task`, `wasm_run`.
