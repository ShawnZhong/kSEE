# The runtime and the page

## One module, in the kernel's memory

crates/ksee-runtime is Rust compiled to wasm (wasm-bindgen, with atomics).
The page and **every Worker instantiate it on the kernel's shared memory**:
its static data sits in the 16 MiB below the kernel image
(`CONFIG_WASM_HOST_MEMORY`), its heap grows above the kernel's RAM, and
wasm-bindgen gives each instance its own stack. So:

- the runtime reads kernel objects and the trace ring **directly**, as
  memory, with no copying and no messages;
- the page and the Workers share the runtime's statics (state.rs): the
  kernel's tables, the addresses of its globals, the console's output ring,
  whether it halted. What the page touches is lock-free: it may not block.

One module rather than two (a worker crate and a page crate) because of
this sharing: two modules would each have their own statics and allocator
in the same memory.

## Who runs what

```text
page (main thread)                      Workers
──────────────────                      ───────
web/ksee.js: create the memory,         web/worker.js: init the runtime on
  init the runtime, Machine.start         the memory, worker_entry(msg)
machine.rs: boot, console, record,      worker.rs: the timer, or a task:
  run programs, snapshots                 instantiate the kernel, run it
replay.rs: views of the recording       imports.rs: the kernel's imports
  (ksee_core::view, as JS values)
                    state.rs: statics shared by both
```

**Booting** (`Machine::start`): fetch and compile vmlinux, parse its
`ksee.probes`/`ksee.types`, put the initramfs at the top of RAM, and
instantiate the kernel once on the page, which runs its start function
(initializing its data) and gives the addresses of its exported globals.
The page never runs kernel code after that. Then it starts the timer Worker
and the boot Worker.

**Messages** are only for creating Workers. The page's `spawner` creates a
Worker and posts it `{ role, the kernel module, the runtime module, the
memory, task, sp }`. A kernel Worker's `spawn_task` import posts
`{ task, sp }` to the page, whose spawner creates that task's Worker (only
the page keeps them all, so that it can stop them at a halt). Everything
else goes through memory: the page raises interrupts (a bit and a notify),
writes `wasm_run`, and reads the console ring.

**Imports**: the frequent ones (`console_write`, `time_ns`,
`ksee.bulk`, ...) are the runtime's own wasm exports, so the kernel calls
them directly; the rest (Workers, user programs' modules and memories) are
closures reaching Web APIs.

## The page

web/ is plain ES modules, no framework or bundler; Monaco (vendored) for
source. Rust does the work: JS only lays it out.

| Module | Does |
|---|---|
| app.js | wires the panels to the Machine; the state (idle, recording, replay, halted); the cursor; line steps and Play |
| timeline.js | the overview, the task strip, the stack chart (a flame graph: outermost at the bottom); zoom, pan |
| source.js | the source at the cursor (Monaco): lines that ran, the function, hover values, Watch, definitions |
| callstack.js | the Stack: calls with arguments and locals, watches, ✎ who wrote a value |
| examples.js | the examples: the boot's recording, or a program run recorded; their stops |
| ksee.js, worker.js | booting; each Worker's script |

Every view asks the Machine for the moment at the cursor: `at(i)`,
`call_stack(i)`, `line_steps`, `chart`, `memory_at(i)` then `object` /
`global` (memory as it was), `last_write`.

## Tests

- `cargo test -p ksee-core`: the rebuilding, on made-up records.
- tests/web.js (Deno, with the page's own ksee.js): boots the kernel, runs
  programs recorded, and checks what the recording shows: syscalls,
  stacks with arguments, lines, memory as it was, who wrote a value, tasks.
- tests/ui.js: the page in headless Chromium over CDP: the boot, then the
  read example: walks its stops, hovers, plays; fails on any exception. Saves
  web/ui.png.

`./ksee.sh test --ui` runs all three (`$CHROME`: a headless Chromium).
