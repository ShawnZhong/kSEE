# kSEE design

kSEE is a browser-based system for interactively exploring the execution of
the **real Linux kernel**: observe what Linux is doing and connect execution
to Linux source code, kernel objects/state, semantic events, and causal
history.

## Scope and positioning

- kSEE is **generic** Linux execution visualization, not a scheduler tool.
- **kSTEP** stays separate: scheduler-specific deterministic testing/control.
  It can later plug into kSEE as an optional scheduler-control backend
  (inject events, control the virtual clock, deliver ticks/IPIs). kSEE must
  work without it, and `arch/wasm` must not be scheduler-specific.
- The first prototype may be scheduler-heavy; a second demo (e.g. `read()` →
  VFS → page cache → filesystem) establishes generality.
- Not "Linux compiled to Wasm" (prior work: see [related/](related/)) and not
  "scheduler visualization" (KernelShark, SchedViz, ...). The contribution is
  interactive exploration of the real kernel that tightly correlates
  **source, live kernel objects, semantic state transitions, causality and
  history**, zero-install in a browser. Where xv6 simplifies the OS so humans
  can understand it, kSEE keeps real Linux and simplifies/explains the
  execution that matters. The key question kSEE answers is **why**: why did
  this task run, this syscall block, this lock contend, this I/O happen.

## Architecture

```text
Linux source → Clang/LLVM → arch/wasm → vmlinux.wasm → browser runtime
             → kSEE instrumentation → interactive UI
```

Linux compiles directly to Wasm through a minimal `arch/wasm` (no CPU
emulator underneath). Initial configuration:

```text
wasm32, CONFIG_MMU=n, CONFIG_SMP=n (NR_CPUS=1), CONFIG_PREEMPT_NONE,
no modules/PCI/ACPI/networking, initramfs/tmpfs, simple console,
simple virtual timer, minimal userspace, asm-generic wherever possible
```

### NOMMU

Wasm linear memory has no Linux-controlled MMU (no PTEs, TLB, faults, COW),
so `CONFIG_MMU=n`. That is adequate for scheduler, syscalls, sleep/wakeup,
locks/waitqueues, VFS, filesystems, page cache, kernel allocation, block I/O.
It is not faithful for `fork()`/COW, page tables, TLBs, demand paging or full
`mmap()`. MMU support (e.g. software translation) is not a prerequisite.

### Tasks: one Worker per `task_struct`

Wasm cannot save/restore its execution stack, so each Linux schedulable task
gets its own Web Worker, whose hidden Wasm stack is the task's continuation.
A context switch hands a **CPU token** from one Worker to another and parks
the old one; when rescheduled, a Worker resumes from its existing stack. No
Asyncify is needed for ordinary scheduling.

```text
schedule A→B on CPU0: transfer CPU0 token A→B, wake B, park A
```

All task Workers instantiate the same kernel module against one **shared
kernel `WebAssembly.Memory`** (SharedArrayBuffer) holding all kernel state.
Userspace may use separate per-process memories; `copy_{to,from}_user()`
become runtime hooks into the current task's user memory.

### SMP (later)

N task Workers, M CPU tokens: M Workers run concurrently, N−M are parked.
This gives real per-CPU runqueues, migration, affinity, load balancing,
spinlocks, atomics, cross-CPU wakeups, IPIs, RCU. Start at `NR_CPUS=1` but
keep runtime structures (`HostCPU{cpu_id, current_task, pending_irqs}`,
`HostTask{tid, worker, cpu, state}`) so going to 2 needs no redesign.

### Preemption

No asynchronous browser-level preemption initially (it would also hurt
determinism). `sched_yield()` is **not** a substitute for preemption. Keep
Linux's sequence: timer/event → `scheduler_tick()` → `resched_curr()` →
`TIF_NEED_RESCHED` → legal reschedule point → `__schedule()`, delivered at
safepoints. Progression: explicit safepoints → compiler-inserted userspace
safepoints → compiler-inserted legal kernel safepoints → finer only if needed.

### fork

Deferred. Possible later: Asyncify **userspace only**; at a clean syscall
boundary after `copy_process()`, unwind the user continuation into memory,
create the child Worker and rewind both (parent gets PID, child 0). Never
clone a kernel continuation. Under NOMMU this is still not real COW fork.
The MVP uses pre-created tasks or a simple spawn.

### No snapshots / reverse execution

Linux always runs forward. kSEE records an append-only semantic history and
the UI scrubs it ("visual time travel"), applying/reversing state deltas on a
frontend shadow model.

## Instrumentation

**Semantic history**: append-only events with state deltas and source, e.g.

```text
event 104 SCHED_SWITCH  CPU0 current A→B; A RUNNING→RUNNABLE;
                        B RUNNABLE→RUNNING; rq0 before→after;
                        source kernel/sched/core.c:...
```

**Hierarchical tracing** to bound overhead/perturbation:

| Level | What |
|---|---|
| 0 | semantic events: sched_switch, wakeup, syscall, IRQ, ... |
| 1 | selected function entry/exit |
| 2 | basic-block/branch history for a selected subsystem/function |
| 3 | selected object/memory mutation |

**Source mapping**: an LLVM pass assigns a `LocationID` to selected
`DILocation`s and inserts `__ksee_location(ID)`; a sidecar maps ID → file,
line, column, function, inline context. The runtime only emits compact IDs;
DWARF stays available for arbitrary inspection.

**Objects**: BTF decodes kernel structs into a browsable object graph
(pointers are edges), with progressive disclosure concept → object → fields →
raw memory. Curate `task_struct`, `sched_entity`, `rq` first; later `file`,
`inode`, `dentry`, `address_space`/folio. Stable identity via
`ObjectID = address + allocation generation` (allocators reuse addresses).

**Causality**: syscall instance/root IDs propagate through async mechanisms
(bio, work_struct, timer, wake events carry provenance), distinguishing
synchronous duration from downstream effects, e.g. `read()` → page-cache miss
→ bio → sleep … IRQ → bio completion → wake → schedule → read resumes.

## UI

The kernel is live; the page is in one of three states, shown in its
header: **LIVE** (running; the history follows now), **PAUSED** (the
debugger stopped it; objects are exact), **REPLAY** (the cursor is in the
recorded past; the kernel goes on). One cursor; every panel shows its
moment.

```text
┌─────────────────────────────────────────────────────────────┐
│ History: a lane per task, syscalls as bars                  │
│ Flame chart of the selected syscall (calls by depth)        │
│ ⏮ ◀ ▶ Play ▶| ⤼ Now   speed                                 │
├──────────────┬──────────────────────────────┬───────────────┤
│ Syscall cards│ Call stack (args when paused)│ Objects       │
│ (typed forms,│ Source (whole file, lines    │ (live memory) │
│ /init makes  │ that ran, the cursor)        │ Syscalls, Con.│
│ them)        │                              │               │
└──────────────┴──────────────────────────────┴───────────────┘
```

- **Cards** are how the page talks to the kernel: a form per syscall built
  from its DWARF signature (fds by name, flag chips, struct fields,
  outputs decoded), no language to learn. /init makes each call in a real
  process, over the /dev/ksee channel (web/calls.js, user/init.c); the
  debugger never stops in that channel's own traffic.
- **Calls** come from the trace: every block, and every return, is
  recorded, so the call tree is rebuilt exactly (ksee_core::stack). While
  paused, the plugin's frames on the kernel stack give the call stack with
  arguments, decoded by their types.
- **Play** goes line by line: through the recorded trace (REPLAY), or by
  running the kernel a line at a time on the stopped task (PAUSED). Step
  into stops the kernel where a card's syscall starts. The kernel's clock
  stands still while it is paused.

Scrubbing changes the visualization's historical state, not the machine.
Primary question: **what changed, why, and where in Linux source?**

## Demos

1. **Scheduler**: 2–3 pre-created tasks; B wakes → `try_to_wake_up()` →
   `enqueue_task()` → `check_preempt_curr()` → need_resched → `__schedule()`
   → B runs. Later 2 CPUs for migration/load balancing/IPIs/spinlocks.
2. **Non-scheduler**: `read()`/`write()` through VFS, page cache, filesystem
   (and JBD2/block layer).

## Explicitly deferred

MMU, COW/POSIX fork, full exec environment/shell, PCI, ACPI, networking,
large device ecosystem, asynchronous preemption, production SMP,
snapshot/restore, reverse execution, full instruction tracing.

## Phases

| Phase | Goal |
|---|---|
| 0 | minimal `arch/wasm`; Linux boots; console works |
| 1 | one task, basic syscall, source/event tracing |
| 2 | multiple task Workers, one CPU token, sleep/wakeup/context switch |
| 3 | scheduler visualization: task_struct/rq, event history, source correlation |
| 4 | generic instrumentation API, BTF object browser, causal history |
| 5 | VFS read/write demo |
| 6 | 2 logical CPUs: SMP scheduler, migration, spinlocks, IPIs |
| later | compiler safepoints, kSTEP integration, fork/Asyncify, networking, MMU |

**First milestone**: the smallest `arch/wasm` + browser runtime that boots
Linux and shows two tasks sleeping, waking and context-switching while
emitting source-correlated semantic events to a browser UI.
