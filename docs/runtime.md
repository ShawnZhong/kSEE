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
web/ksee.js      init the module; Machine.start (then the page mounts: ksee-page)
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
raises interrupts (a bit, and a wake-up of idle), and asks a runner (the program,
or a thread or child of it: see [kernel.md](kernel.md)) for a syscall
(a line of the program, `Machine.run`: it writes the call into the runner's
request and rings the runner, which waits in a futex on its number; the
runner records the syscall it then makes, `ksee.begin` to `ksee.end`); it
reads the console's output. `Machine.quiet()` is a promise of
the kernel **quiet** (idle, until the page's input), with the calls
finished meanwhile: returned (`ksee.end`, with the result), ended by an
execve (the new program's `ksee.ready`) or by their task's exit.
A call may not have finished then: it blocks (a futex's wait, a pipe's
read), and finishes as a later call wakes it. (While a call is under way,
idle skips the clock to the next timer only if the call's runner sleeps on
a timer of its own: ksee-core's arch::timer_wakes reads the kernel's timers for one; see
[kernel.md](kernel.md).) `Machine.syscall_of(id)` finds a call's syscall
in the recording. A boot is done when the program (/bin/prog) is ready and the kernel quiet
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

The page is Rust too: crates/ksee-page, in Dioxus, built into the same
wasm module as the runtime, so it calls the Machine directly, Rust to
Rust (`Machine::make`, `view`, `at_moment`: a view with memory as it was
at a moment). web/ is its frame: index.html (the style) and ksee.js (it
boots the kernel, `Machine.start`, then `mount`s the page, handing it the
Machine). The page's state is three things, in Dioxus signals: the call
selected, the cursor in it (a line it ran), and the frame chosen on the
Stack; what they make (the call's lines, its chart, the stack at the
cursor) is computed once each time they change (memos).

| Module | Does |
|---|---|
| lesson.rs | the lessons: the bar (every one by chapter, ◀ ▶), a lesson as a notebook (each program line, what it did, the variables its call changed, the paragraphs about it), the runner (a line at a time on a fresh kernel, the kernel quiet after a call), links (a call, a function in it, a tab; a try), the URL |
| editor.rs | a line written in place: what fits at the cursor (`Machine::complete`), the signature, why a line cannot be made (`Machine::can_make`) |
| chart.rs | the call selected as a stack chart (`View::chart`): DOM bars, a run of a call over its columns each (a click: the cursor there, that frame); the task strip; find a function |
| stack.rs | the transport (where the cursor is; stepping into, over, out: `view::step`); the Stack: frames, values that open (as memory was then) |
| source.rs | the source of the frame shown: its function's lines (the site's src/), the line it runs marked, those that ran in the call |
| panels.rs | the tabs: Stack, Objects (what the call changed), Tasks (each task_struct to open), Console |
| state.rs | the state and what it makes |
| text.rs | a lesson's paragraphs: text, `code`, links |

A program is a small language, parsed, evaluated and laid out in the core
(crates/ksee-core/src/program/), a line made by `Machine.run`, checked
first by `Machine.check` (the editor's: nothing made). Its names are the
Machine's: each line sees those of the lines before it (a variable's, a
call's result once it returns; a clone's, its child runner as it starts:
`Machine.names_before`, the completions' and the hovers'). A call's arguments are numbers, the kernel's constants
with `| ^ & << >> + - * /`, `- ~`, strings, results by name,
and compound literals laid out by the parameter's type (a struct,
positional or by member; an array; argv for a `char **`); and a line can
declare a variable of the program's as in C (`char buf[64]`, `struct
__kernel_timespec ts = {0, 30000000}`: a type of the kernel's), which
goes in the program's own memory (user/prog.c's heap, in
its process; a variable of a page or more on a page of its own, as memory
the kernel pins must be), its name then its address (`&ts` too; `fds[1]` an element;
`rings->sq.tail`, through a result typed as a pointer; a line `target =
value` writes one, recorded, with what a declaration's value may be: an
array whole too),
`Machine.show` printing it as C writes it. A call's strings and compound literals go
in the same memory, as unnamed variables (a C literal's, there for as long
as the program).
ksee pack evaluates the UAPI headers' constants with the same evaluator.

Every view asks the Machine for the moment at the cursor: `at(i)` (every
record located: a store at its line, a return at the line it returned
from), `call_stack(i, nth)`, `line_steps`, `chart`,
`tasks(i)` (each task, where it is), `memory_at(i)` then `object` /
`global` (memory as it was), `origin` (who last wrote a value, back through its copies), `objects`
(what a call did) and `notes` (the source's notes by line), their stores
(named once per span and snapshot) named as the code names what they
hit: the structs the calls' arguments point to, the task, locals, the
kernel's globals by address, the machine's own variables apart: "file
in vfs_read"; else by its address; `raw` (records as text), `run_of` (a line's next or
last run), `definition`.

## Tests

- `cargo test -p ksee-core`: the rebuilding, on made-up records.
- tests/web.js (Deno, with the page's own ksee.js and tests/kernel.js, a
  runner of programs as the page's): boots the kernel, has
  the program make syscalls, recorded (and every example's), and checks what the
  recordings show: stacks with arguments, lines, memory as it was, who
  wrote a value, tasks, the examples' links.
- tests/ui.js: the page in headless Chromium over CDP, a script from
  tests/ui/ run in it (after lib.js): read.js, the read example (its
  subject call selected, its links, each landing in its function; stepping
  into, over, out, and by key; a click on a bar; find; Objects, a
  task_struct opened; a line changed, a try and back; the next lesson);
  back.js, a changed program from the URL; pipe.js, two processes, a call
  that blocks, what calls changed; phone.js, the read example at 390x844,
  the panes one under another. Fails on any exception; saves a screenshot in
  target/.

`./ksee.sh test --ui` runs them all, the page's too (`$CHROME`: a headless
Chromium).
