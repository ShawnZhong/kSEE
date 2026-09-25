# kSEE design

## Goal

**kSEE records what the real Linux kernel does, down to every line it runs
and every value it changes, and lets you explore the recording in time.**

The question it answers: *what did the kernel do here, why, and where in the
source?* It runs in a browser, with nothing to install: Linux itself is
compiled to WebAssembly (no emulator), and instrumented by the compiler.

**Who it is for**

- *Learners*: how an OS really works, on real Linux instead of a teaching
  kernel. They start from **examples**: a question ("What happens in
  read()?", "Directories on ext4"), a few syscalls that ask it, made one
  at a time, and stops in their recordings that explain what happens.
- *Kernel developers*: what a code path really does, on a real kernel they
  can read and change. They make **their own syscalls**, or **record** the
  kernel as it runs.

**What it is not**: a live debugger (it replays; it does not pause the
kernel), a syscall playground, a profiler, a full Linux system (no MMU, no
SMP, no fork: see Limits).

## The experience

1. **Make syscalls**, each recorded: an example's (the page makes the
   first's), your own (by name, with its parameters as fields), or record
   the kernel for a while (● Record). A syscall takes milliseconds.
2. **Explore the recording**, at one cursor:
   - *time*: a timeline of the call stack (who ran, what they called);
     drag, step line by line, Play;
   - *code*: the source line at the cursor, the lines that ran;
   - *values*: the call stack with each call's arguments and locals,
     watches, and a name's value on hover; memory as it was at the cursor;
     ✎: which code last wrote a value.
   An example's stops are places in the recording, with a note each: Next goes
   to the next one.

```text
┌─────────────────────────────────────────────────────────────┐
│ Timeline: overview · call stacks over time · tasks · cursor │
├──────────────┬──────────────────────────────┬───────────────┤
│ Examples     │ The stop's note              │ Stack: calls, │
│              │ Source at the cursor         │ arguments,    │
│              │ (lines that ran)             │ locals,       │
│              │                              │ watches       │
└──────────────┴──────────────────────────────┴───────────────┘
```

## How it works

```text
linux/ (arch/wasm) ──clang + plugin──▶ vmlinux (wasm) ──────┐
user/ (init, examples, tests: C on nolibc) ─▶ initramfs ────┼─▶ web/ ─▶ browser
crates/ksee-runtime (Rust → wasm) ──────────────────────────┘
```

- **The kernel** ([kernel.md](kernel.md)): a minimal `arch/wasm` for Linux
  7.2. Each task runs in its own Web Worker, all on one shared kernel
  memory; a context switch hands a CPU token from one Worker to the next.
  /init makes the syscalls the page asks for (the doorbell: console input).
- **The recording** ([recording.md](recording.md)): an LLVM pass
  instruments every function the same way (blocks, calls with arguments,
  returns, locals, stores with old values) into one ring.
- **Rebuilding the past** ([replay.md](replay.md)): from the records alone,
  crates/ksee-core rebuilds which task ran, call stacks, lines within
  blocks, syscalls, and memory as it was at any record.
- **The runtime and the page** ([runtime.md](runtime.md)): one Rust wasm
  module, shared by the page and the Workers, boots the kernel and reads it
  directly; the page (web/) lays out what the core computes.
- **The build** (crates/ksee-cli, `./ksee.sh`): the plugin, the kernel, the
  runtime, /init, and the page's files (types from DWARF, the
  sources, Monaco), into web/.

The choices that shape it:

- **Real Linux, minimal port.** Generic kernel code wherever it works;
  arch/wasm only where wasm differs.
- **The compiler records, uniformly.** No tracepoints or per-subsystem
  hooks, so no part of the kernel is out of view.
- **Record, then replay.** The kernel is never paused; everything the page
  shows is rebuilt from a recording, so any moment can be revisited.
- **Rust does the work, JS lays it out**, and what needs no browser is in
  ksee-core, tested natively.

## Limits

- No MMU: no page tables, faults, copy-on-write or demand paging; no user
  fork (one program: /init).
- One CPU; no preemption (the kernel reschedules at its usual points).
- Arguments are 32 bits (wider ones lose their high half); locals are only
  those in memory; values in wasm locals are not recorded.
- A recording is a ring: the oldest records go when it is full.
- No console input.
