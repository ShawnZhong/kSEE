# The runtime and the page

## One thread, one memory

crates/ksee-runtime is Rust compiled to wasm (wasm-bindgen), on the page's
own thread. The kernel is one wasm instance on the runtime's memory: its
RAM above the runtime's static data and stack (`CONFIG_WASM_HOST_MEMORY`),
the runtime's heap above its RAM. So the runtime reads kernel objects and
the trace ring **directly**, as memory, with no copying and no messages.

The kernel's tasks are call stacks of that instance, which JSPI suspends
and resumes (sched.rs; see [kernel.md](kernel.md)). No host code runs while
the kernel does: the page's JavaScript runs while it is suspended (a task
switch, idle), so the page stays responsive between the kernel's bursts
(a boot is ~0.3 s, a call milliseconds). No Workers, no shared memory, so
no cross-origin isolation either. It needs JSPI: Chrome and Edge 137,
Firefox 153, Safari 27.

```text
web/ksee.js      init the runtime; Machine.start
machine.rs       boot (a new kernel instance on wiped memory), restart,
                 console, snapshots, syscalls by the runners (recorded)
sched.rs         the tasks: switch_task, idle (suspending), spawn_task
imports.rs       the kernel's imports (the frequent ones: direct exports)
programs.rs      user programs (in kernel memory), their tasks' stack
                 pointers; the runners, which make the page's calls
replay.rs        views of the recording (ksee_core::view, as JS values)
state.rs         the machine's state in the runtime
```

**Booting** (`Machine.start`): grow the memory to the kernel's RAM, fetch
and compile vmlinux, parse its `ksee.probes`/`ksee.types`; then each boot
wipes the kernel's memory, puts the initramfs at the top of RAM, and
instantiates the kernel (which initializes its data), and calls its
`boot` export (JSPI: it runs until it first suspends). **Restart**: stop
the machine (its tasks dropped; a stopped machine's stack that suspends is
never resumed), let what it had under way end, and boot again.

**The page and the kernel** talk through memory and promises: the page
raises interrupts (a bit, and a wake-up of idle), and asks a runner (/init,
or a thread or child of it: see [kernel.md](kernel.md)) for a syscall
(`Machine.call(name, args, tid)`: it rings the runner, which waits for
SIGUSR1); it reads the console's output. `Machine.quiet()` is a promise of
the kernel **quiet** (idle, until the page's input: the recording stops
there), with the calls finished meanwhile: returned (`ksee.done`), ended
by an execve (the new program's `ksee.ready`) or by their task's exit.
A call may not have finished then: it blocks (a futex's wait, a pipe's
read), and finishes as a later call wakes it. A boot is done when /init
is ready and the kernel quiet (`Machine.start` and `restart` resolve
then).

**Deterministic.** Nothing of the host's gets in but the page's calls: the
kernel's clock is virtual (see [kernel.md](kernel.md)), the page's input
arrives only while the kernel idles, and one thread runs one task at a
time. So the same calls on a fresh kernel make the same recording, record
for record (tests/web.js checks it): going back (making calls again) comes
back to the very same state.

**Imports**: the frequent ones (`console_write`, `time_ns`, `spawn_task`,
`ksee.bulk`, ...) are the runtime's own wasm exports, so the kernel calls
them directly; the rest (promises, /init's module and memory) are closures
reaching Web APIs.

## The page

web/ is plain ES modules, no framework or bundler; Monaco (vendored) for
source. Rust does the work: JS only lays it out.

| Module | Does |
|---|---|
| app.js | wires the panels to the Machine; the state (idle, replay, halted); the cursor; line steps and Play |
| timeline.js | the overview, the task strip, the stack chart (a flame graph: outermost at the bottom); zoom, pan |
| source.js | the source at the cursor (Monaco): lines that ran, the function, hover values, definitions |
| callstack.js | the Stack: calls with arguments and locals, the task (current) |
| examples.js | the examples: the program (syscalls the runners make, all as it opens, each recorded; a call a line, each editable), the tutorial (its stops) |
| calledit.js | a call written as in C: completions, the signature |
| args.js | a syscall's arguments, written as in C, with the kernel's constants |
| ksee.js | booting |

Every view asks the Machine for the moment at the cursor: `at(i)`,
`call_stack(i)`, `line_steps`, `chart`, `memory_at(i)` then `object` /
`global` (memory as it was), `last_write`.

## Tests

- `cargo test -p ksee-core`: the rebuilding, on made-up records.
- tests/web.js (Deno, with the page's own ksee.js): boots the kernel, has
  /init make syscalls, recorded (and every example's), and checks what the
  recordings show: stacks with arguments, lines, memory as it was, who
  wrote a value, tasks, the examples' stops.
- tests/ui.js: the page in headless Chromium over CDP: the first example
  (it runs by itself), then the read example: walks its stops, hovers, plays; fails on any exception. Saves
  target/ui.png.

`./ksee.sh test --ui` runs all three (`$CHROME`: a headless Chromium).
