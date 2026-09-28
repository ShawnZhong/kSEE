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
(`Machine.call(name, args, tid)`: it writes the call into the runner's
request and rings the runner, which waits in a futex on its number; the
runner records the syscall it then makes, `ksee.begin` to `ksee.end`); it
reads the console's output. `Machine.quiet()` is a promise of
the kernel **quiet** (idle, until the page's input), with the calls
finished meanwhile: returned (`ksee.end`, with the result), ended by an
execve (the new program's `ksee.ready`) or by their task's exit.
A call may not have finished then: it blocks (a futex's wait, a pipe's
read), and finishes as a later call wakes it. (While a call is under way,
idle skips the clock to the next timer only if the call's runner sleeps on
a timer of its own: sched.rs reads the kernel's timers for one; see
[kernel.md](kernel.md).) `Machine.syscall_of(id)` finds a call's syscall
in the recording. A boot is done when /init is ready and the kernel quiet
(`Machine.start` and `restart` resolve then).

**Deterministic.** Nothing of the host's gets in but the page's calls: the
kernel's clock is virtual (see [kernel.md](kernel.md)), the page's input
arrives only while the kernel idles, and one thread runs one task at a
time. So the same calls on a fresh kernel make the same recording, record
for record (tests/web.js checks it): going back (making calls again) comes
back to the very same state.

**Imports**: the frequent ones (`console_write`, `time_ns`, `spawn_task`,
`ksee.bulk`, ...) are the runtime's own wasm exports, so the kernel calls
them directly; the rest (promises, programs' modules) are closures
reaching Web APIs.

## The page

web/ is plain ES modules, no framework; CodeMirror for the source, one
bundle the build makes with `deno bundle` (target/site/codemirror.js).
Rust does the work: JS only lays it out.

| Module | Does |
|---|---|
| app.js | wires the panels to the Machine; the status (idle, replay, halted); the cursor; stepping (in time: the selected call's lines, or every line; in depth: the calls above and below), run to a line; the keys |
| timeline.js | the overview, the task strip, the stack chart (a flame graph: outermost at the bottom), on an axis of steps (blocks run: `Machine.steps`); zoom, pan |
| source.js | the source at the cursor (CodeMirror): lines that ran, the function, the cursor's line's values at its end, what each line stored so far at its end (a link to the store), hover values, definitions |
| callstack.js | the Stack: calls with arguments and locals, the task (current); the right column's tabs |
| tree.js | values as trees (the Stack's): pointers open into what they point to, structs into their fields; who last wrote a value; a value's writes, a row under it |
| records.js | the Trace tab: the raw trace, a line a record (what the views are built from) |
| stores.js | the Stores: every store the selected call made, a row each in order, each a link; the store at the cursor marked |
| objects.js | the Objects: the objects the selected call wrote as boxes (their members written), their pointers to each other as arrows (`Machine.objects`), as memory was at the cursor |
| trace.js | the Calls: the calls the selected call made, in order, each with what it returned, inlined code among them (a chain of single inlined calls on one row); ▸ opens a call's own calls |
| examples.js | the examples: the program (syscalls the runners make, all as it opens, each recorded; a call a line, each editable), the tutorial (its stops); the URL (`#ex=NAME&p=CALLS&stop=N`: the program if changed, its calls separated by `;`) |
| calledit.js | a call written as in C: completions, the signature |
| args.js | a program's line: `name = call(args)`, `tid: call(args)` (the arguments themselves are ksee-core's args.rs: `Machine.arg`) |
| ksee.js | booting |

A call's arguments are a small language, parsed and laid out in the core
(crates/ksee-core/src/args.rs, `Machine.arg`, `Machine.value`): numbers,
the kernel's constants with `| << + -`, strings, buffers to fill (`[64]`),
results by name or `$n`, and compound literals laid out by the parameter's
type (a struct, positional or by member; an array; argv for a `char **`).
ksee pack evaluates the UAPI headers' constants with the same evaluator.

Every view asks the Machine for the moment at the cursor: `at(i)` (every
record located: a store at its line, a return at the line it returned
from), `call_stack(i, nth)`, `line_steps`, `chart`,
`memory_at(i)` then `object` /
`global` (memory as it was), `last_write`, `writes` (an object's history:
every store to it in its lifetime, a local's frame or the whole
recording: the member written, from what to what), `stores` (a span
of records' stores, in order, each named as the code names what it hit:
the structs the calls' arguments point to, the task, locals and frames,
the kernel's globals by address, the machine's own variables apart:
"vfs_read's file"; else by its address), `raw` (records as text), `run_of` (a line's next or
last run), `definition`.

## Tests

- `cargo test -p ksee-core`: the rebuilding, on made-up records.
- tests/web.js (Deno, with the page's own ksee.js): boots the kernel, has
  /init make syscalls, recorded (and every example's), and checks what the
  recordings show: stacks with arguments, lines, memory as it was, who
  wrote a value, tasks, the examples' stops.
- tests/ui.js: the page in headless Chromium over CDP, a script from
  tests/ui/ run in it (after lib.js): read.js, the read example (its
  stops, stepping, run to a line, the chart, typing and changing calls,
  another example); back.js, a changed program from the URL; pipe.js, two
  processes, calls that block. Fails on any exception; saves a screenshot
  in target/.

`./ksee.sh test --ui` runs all three (`$CHROME`: a headless Chromium).
