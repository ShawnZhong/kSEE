# kSEE design

## Goal

**kSEE records what the real Linux kernel does, down to every line it runs
and every value it changes, and lets you explore the recording in time.**

The question it answers: *what did the kernel do here, why, and where in the
source?* It runs in a browser, with nothing to install: Linux itself is
compiled to WebAssembly (no emulator), and instrumented by the compiler.

**Who it is for**

- *Learners*: how an OS really works, on real Linux instead of a teaching
  kernel. They start from **examples**, grouped by topic (start here;
  files, in memory, then on ext4; time; threads and processes): a
  question ("What is a file descriptor?", "What happens in
  read()?", "What is a directory?", "How do two processes talk through a
  pipe?"), a program of a few syscalls that asks it, and a short tutorial:
  stops in the recording that explain what happens. The first, printing
  "hello", is also a first look at kSEE itself.
- *Kernel developers*: what a code path really does, on a real kernel they
  can read and change. They make **their own syscalls**.

**What it is not**: a live debugger (it replays; it does not pause the
kernel), a syscall playground, a profiler, a full Linux system (no MMU, no
SMP, no fork (threads and vfork, though): see Limits).

## The experience

1. **Make syscalls**, each recorded: an example's program (the page makes
   them as it opens), changed or added to (calls written as in C). A
   syscall takes milliseconds.
2. **Explore the recording**, at one cursor, as in a debugger that goes
   both ways:
   - *time*: a timeline of the call stacks (who ran, what they called);
     step by line, over calls or out of them, forwards or back; run to a
     line (its next run, or its last); Play;
   - *code*: the source line at the cursor, the lines that ran;
   - *values*: the call stack with each call's arguments and locals,
     the task running, and a name's value on hover (who last wrote it);
     memory as it was at the cursor.
   An example's tutorial is its stops, places in the recording with a note
   each: ◀ ▶ (or [ ]) go from one to the next.

```text
┌──────────────┬──────────────────────────────────────────────┐
│ Example:     │ Timeline: overview · call stacks · tasks     │
│  question    │ ◀ ▶ Play · step by line / over / out         │
│  tutorial    ├───────────────────────────────┬──────────────┤
│  (its stops) │ Source at the cursor          │ Stack: calls,│
│  program     │ (the lines that ran)          │ arguments,   │
│  (its calls) │                               │ locals,      │
│ Console      │                               │ current task │
└──────────────┴───────────────────────────────┴──────────────┘
```

## How it works

```text
linux/ (arch/wasm) ──clang + plugin──▶ vmlinux (wasm) ──────┐
user/ (/init: C on nolibc; the examples) ─▶ initramfs ────┼─▶ site ─▶ browser
crates/ksee-runtime (Rust → wasm) ──────────────────────────┘
```

- **The kernel** ([kernel.md](kernel.md)): a minimal `arch/wasm` for Linux
  7.2. One wasm instance, on the page's thread; a task is a call stack of
  it, which the host suspends and resumes at a context switch (JSPI).
  /init makes the syscalls the page asks for, and so do its threads and
  children (runners; the doorbell: a signal).
- **The recording** ([recording.md](recording.md)): an LLVM pass
  instruments every function the same way (blocks, calls with arguments,
  returns, locals, stores with old values) into one ring.
- **Rebuilding the past** ([replay.md](replay.md)): from the records alone,
  crates/ksee-core rebuilds which task ran, call stacks, lines within
  blocks, syscalls, and memory as it was at any record.
- **The runtime and the page** ([runtime.md](runtime.md)): one Rust wasm
  module, on the kernel's memory, boots the kernel and reads it
  directly; the page (web/) lays out what the core computes.
- **The build** (crates/ksee-cli, `./ksee.sh`): the plugin, the kernel, the
  runtime, /init, and the page's files (types from DWARF, the
  sources, Monaco), with web/ (the page), into target/site/.

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

- No MMU: no page tables, faults, copy-on-write or demand paging; no
  fork (threads and vfork, which share memory; one program: /init).
- One CPU; no preemption (the kernel reschedules at its usual points).
- Arguments are 32 bits (wider ones lose their high half); locals are only
  those in memory; values in wasm locals are not recorded.
- A recording is a ring: the oldest records go when it is full.
- No console input; no user signal handlers (a caught signal kills the
  task).
