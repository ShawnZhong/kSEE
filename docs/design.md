# kSEE design

## Goal

**kSEE records what the real Linux kernel does, down to every line it runs
and every value it changes, and lets you explore the recording in time.**

The question it answers: *what did the kernel do here, why, and where in the
source?* It runs in a browser, with nothing to install: Linux itself is
compiled to WebAssembly (no emulator), and instrumented by the compiler.

**Who it is for**

- *Learners*: how an OS really works, on real Linux instead of a teaching
  kernel. They start from **tours**: a question ("What happens in
  read()?"), a small C program that asks it, and stops that explain what
  happens.
- *Kernel developers*: what a code path really does, on a real kernel they
  can read and change. They run **their own programs**, or **record** the
  kernel as it runs.

**What it is not**: a live debugger (it replays; it does not pause the
kernel), a syscall playground, a profiler, a full Linux system (no MMU, no
SMP, no fork: see Limits).

## The experience

1. **Run** something, recorded: a tour's program, another program, or the
   kernel for a while (● Record). Programs run to the end: a recording
   takes milliseconds.
2. **Explore the recording**, at one cursor:
   - *time*: a timeline of the call stack (who ran, what they called);
     drag, step line by line, Play;
   - *code*: the source line at the cursor, the lines that ran;
   - *values*: the call stack with each call's arguments and locals,
     watches, and a name's value on hover; memory as it was at the cursor;
     ✎: which code last wrote a value.
   A tour's stops are places in the recording, with a note each: Next goes
   to the next one.

```text
┌─────────────────────────────────────────────────────────────┐
│ Timeline: overview · tasks · call stacks over time · cursor │
├──────────────┬──────────────────────────────┬───────────────┤
│ Tours,       │ The stop's note              │ Stack: calls, │
│ programs     │ Source at the cursor         │ arguments,    │
│              │ (lines that ran)             │ locals,       │
│              │                              │ watches       │
└──────────────┴──────────────────────────────┴───────────────┘
```

## The recording

Every kernel function is instrumented the same way by an LLVM pass
(plugin/ksee-trace.cpp) after optimization, and writes to one ring:

- each **basic block** (a call's entry block, with its arguments);
- each **return**, with its value;
- a call's **locals in memory** (their addresses, once they exist);
- each **store**, with the value it overwrites (not to a function's own
  stack slots, except those locals).

The runtime adds a few records (the program's source line before each
syscall; which task a program is). From these alone the host
(crates/ksee-core) rebuilds everything: syscalls (wasm_syscall's calls and
returns), the running task (where __switch_to returns), the call tree and
call stacks with arguments and locals, source lines within blocks (the
plugin's line tables), and memory as it was at any record (the stores after
it, undone from memory as it was when recording stopped).

Recording happens only while asked for (a program runs, or ● Record), so a
recording holds what was asked for, and the kernel runs uninstrumented
otherwise (a check per probe).

## Architecture

```text
linux/ (arch/wasm) ──clang + plugin──▶ vmlinux (wasm) ─┐
user/  (init, tours, tests: C on nolibc) ─▶ initramfs ─┼─▶ web/ ─▶ browser
crates/ksee-runtime (Rust → wasm) ─────────────────────┘
```

- **The kernel** (linux/, branch wasm): a minimal `arch/wasm` for Linux 7.2:
  wasm32, `CONFIG_MMU=n`, one CPU, `PREEMPT_NONE`, an initramfs, a console,
  a timer. Wasm cannot save a call stack, so each task runs in its own Web
  Worker, whose stack is the task's; a context switch hands a CPU token
  from one Worker to the next. All Workers share one kernel memory.
  Programs are wasm modules with their own memory; a syscall is a call
  into the kernel's `syscall` export. The page asks the kernel to run a
  program (arch/wasm/kernel/run.c: a request in kernel memory, and an
  interrupt, the host's only way to wake the kernel); there is no user
  fork on wasm, so the kernel starts it as a usermode helper.
- **The runtime** (crates/ksee-runtime, Rust to wasm, wasm-bindgen): boots
  the kernel in Workers, implements its imports, and serves the page: the
  Machine (snapshots of the recording, and views of it).
- **The core** (crates/ksee-core): decoding and rebuilding, no browser: the
  trace, stacks and lines, memory as it was, kernel types (from DWARF).
- **The build** (crates/ksee-cli, `./ksee.sh`): the plugin, the kernel, the
  runtime, the programs, and the page's files (types from DWARF, the
  sources, Monaco), into web/.
- **The page** (web/): tours and programs, the timeline, the source
  (Monaco), the Stack.

## Limits

- No MMU: no page tables, faults, copy-on-write or demand paging; no user
  fork (programs are started by the kernel).
- One CPU; no preemption (the kernel reschedules at its usual points).
- Arguments are 32 bits (wider ones lose their high half); locals are only
  those in memory; values in wasm locals are not recorded.
- A recording is a ring: the oldest records go when it is full.
