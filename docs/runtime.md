# The runtime and the page

## One thread, one memory

crates/ksee-runtime is Rust compiled to wasm, on the page's own thread.
The kernel is one wasm instance on the runtime's memory: its RAM above
the runtime's static data and stack (`CONFIG_WASM_HOST_MEMORY`), the
runtime's heap above its RAM. So the runtime reads kernel objects and the
trace ring **directly**, as memory, with no copying and no messages.

The kernel's tasks are call stacks of that instance, which JSPI suspends
and resumes (sched.rs; see [kernel.md](kernel.md)). No host code runs
while the kernel does: the page's JavaScript runs while it is suspended (a
task switch, idle), so the page stays responsive between the kernel's
bursts (a boot is ~0.3 s, a call milliseconds). No Workers, no shared
memory, so no cross-origin isolation either. It needs JSPI: Chrome and
Edge 137, Firefox 153, Safari 27.

```text
web/ksee.js      load the module; Machine.start (the page mounted meanwhile: ksee-page)
machine.rs       boot (a new kernel instance on wiped memory), restart,
                 console, snapshots, syscalls by the runners (recorded)
sched.rs         the tasks: switch_task, idle (suspending), spawn_task;
                 the clock, skipped to the timer's deadline
imports.rs       the kernel's imports (the frequent ones: direct exports)
programs.rs      user programs (in kernel memory), their tasks' stack
                 pointers; the runners, which make the page's calls
replay.rs        the last snapshot: views of it (ksee_core::view), at a
                 moment's memory too (at_moment), the page's lookups
state.rs         the machine's state in the runtime
```

**Booting** (`Machine.start`): grow the memory to the kernel's RAM (64
MiB), fetch and compile vmlinux, parse its `ksee.probes`/`ksee.types`;
then each boot wipes the kernel's memory, puts the initramfs at the top
of RAM, and instantiates the kernel (which initializes its data), and
calls its `boot` export (JSPI: it runs until it first suspends).
**Restart**: stop the machine (its tasks dropped; a stopped machine's
stack that suspends is never resumed), let what it had under way end, and
boot again.

**The page and the kernel** talk through memory and promises: the page
asks a runner (the program, or a thread or child of it: see
[kernel.md](kernel.md)) for a syscall (`Machine::make`: it writes the
call into the runner's request and rings the runner, which waits in a
futex on its number; the runner records the syscall it then makes,
`ksee.begin` to `ksee.end`); it reads the console's output.
`Machine::quiet` waits for the kernel **quiet** (idle, until the page's
input), and gives the calls finished meanwhile: returned (`ksee.end`,
with the result), ended by an execve (the new program's `ksee.ready`) or
by their task's exit. A call may not have finished then: it blocks (a
futex's wait, a pipe's read), and finishes as a later call wakes it.
(While a call is under way, idle skips the clock to the next timer only
if the call's runner sleeps on a timer of its own: the runtime's
`timer_wakes` reads the kernel's timers for one; see [kernel.md](kernel.md).)
`Machine::syscall_of(id)` finds a call's syscall in the recording. A boot
is done when the program (/bin/prog) is ready and the kernel quiet.

**Deterministic.** Nothing of the host's gets in but the page's calls:
the kernel's clock is virtual (see [kernel.md](kernel.md)), the page's
input arrives only while the kernel idles, and one thread runs one task
at a time. So the same calls on a fresh kernel make the same recording,
record for record (the kernel's tests check it): going back (making calls
again) comes back to the very same state.

**Imports**: the frequent ones (`console_write`, `time_ns`,
`spawn_task`, `ksee.bulk`, ...) are the runtime's own wasm exports, so
the kernel calls them directly; the rest (promises, programs' modules) are
closures reaching Web APIs.

## The page

The page is Rust too: crates/ksee-page, in Dioxus, built into the same
wasm module as the runtime, so it calls the Machine directly, Rust to
Rust. web/ is its frame: index.html (the style) and ksee.js (it loads the
module, then boots the kernel, `Machine.start`). The page is mounted as
soon as the module is loaded, handed the kernel booting (a promise of the
Machine): the lesson shows meanwhile, its program made once the kernel is
up. The page's state is three things, in Dioxus signals: the call
selected, the cursor in it (a line it ran), and the frame chosen on the
Stack (besides: the chart's window, the moment under the pointer, the
tab); what they make (the call's lines, the chart, the moment shown and
its stack) is computed once each time they change (memos).

| Module | Does |
|---|---|
| lesson.rs | the lessons: their head (every one by chapter, ◀ ▶), a lesson (its paragraphs; its program, each line what it did, in lanes for more than one runner), the runner (a line at a time on a fresh kernel), links (a call, a function in it, a tab; a try), the URL |
| editor.rs | a line written in place: what fits at the cursor, the signature, why a line cannot be made |
| chart.rs | the timeline: the overview, the window as a stack chart, the task strip, the transport; find a function |
| stack.rs | stepping (into, over, out; on into the task's next syscall), the keys; the Stack: frames, values that open (as memory was then), who wrote a value |
| source.rs | the source of the frame shown (the site's src/): the whole file, only the lines in view made; the line it runs, its values; those that ran in the call, what each stored; a name's value, its definition |
| panels.rs | the tabs: Stack, Tasks (a tree, each task_struct to open), Objects (what the call changed), Console, Trace |
| layout.rs | the splits: the lesson's column, the chart's height, the tabs' column, dragged (kept in the browser's storage) |
| state.rs | the state and what it makes |

A program is a small language, parsed, evaluated and laid out in the core
(crates/ksee-core/src/program/), a line made by `Machine::make`, checked
first by `Machine::can_make` (the editor's: nothing made). Its names are
the Machine's: each line sees those of the lines before it (a variable's,
a call's result once it returns; a clone's, its child runner as it
starts: `Machine::names_before`, the completions'). A call's arguments are
numbers, the kernel's constants with `| ^ & << >> + - * /`, `- ~`,
strings, results by name, and compound literals laid out by the
parameter's type (a struct, positional or by member; an array; argv for a
`char **`); and a line can declare a variable of the program's as in C
(`char buf[64]`: a type of the kernel's), which goes in the program's own
memory, its name then its address (`&ts` too; `fds[1]` an element;
`rings->sq.tail`, through a result typed as a pointer; a line `target =
<<<<<<< HEAD
value` writes one, recorded), `Machine::show` printing it as C writes it.
A call's strings and compound literals go in the same memory, as unnamed
variables (a C literal's, there for as long as the program). The build's pack
evaluates the UAPI headers' constants with the same evaluator.

The page asks the views (ksee_core::view, through `Machine::view`, or
`Machine::at_moment` for memory as it was then) for the moment at the
cursor: `at(i)` (every record located), `call_stack(i, nth)`, `lines`,
`chart`, `tasks(i)` (each task, where it is), `object` (memory as it was),
`origin` (who last wrote a value, back through its copies), `objects`
(what a call did) and `notes` (the source's notes by line), `run_of` (a
line's next or last run), `definition`.

## Tests

- `cargo test -p ksee-core`: the rebuilding, on made-up records (and the
  views, the programs, the highlighter, a paragraph's pieces: what the
  page works out is in the core, tested natively; the page is wasm only,
  as the runtime is).
- crates/ksee-page/src/selftest.rs: the kernel's tests (`#selftest`, the
  page not mounted): boots the kernel, has the program make syscalls,
  recorded (and every example's), and checks what the recordings show:
  stacks with arguments, lines, memory as it was, who wrote a value,
  tasks, the examples' links.
- crates/ksee-page/src/uitest.rs: the page's tests, what a user does
  through its DOM: read (its subject call selected, its links, each
  landing in its function; stepping into, over, out, and by key; a click
  on the chart; find; Objects, a task_struct opened, the Trace; a line
  changed, a try and back; the next lesson); back, a changed program from
  the URL; pipe, two processes, a call that blocks, what calls changed;
  phone, the read example at 390x844, the panes one under another.

<<<<<<< HEAD
The kernel's run on `ksee test`, the page's too with `--ui` (a screenshot
each in `target/`), all in headless Chromium (`$CHROME`), in the page
built with them (the selftest feature: target/test/site): the page runs
its test and POSTs `ok` or `fail` and its log to the server, which `ksee`
(crates/ksee-cli/src/chrome.rs) waits for. `ksee shot OUT.png [--at=#HASH]`
looks at the page as it is built (Chrome's own screenshot).
