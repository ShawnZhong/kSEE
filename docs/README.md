# kSEE docs

- [design.md](design.md): the goal, the experience, how it works, limits.

How it works, part by part:

- [kernel.md](kernel.md): Linux on wasm (arch/wasm): a Worker per task,
  interrupts, programs, the console.
- [recording.md](recording.md): what the compiler records, the ring, and
  what the kernel says about itself.
- [replay.md](replay.md): rebuilding the past from the records: tasks,
  stacks, lines, syscalls, memory as it was.
- [runtime.md](runtime.md): the runtime (one wasm module in the kernel's
  memory), the page, the tests.

Background:

- [related/](related/): other Linux-on-Wasm systems (research notes from
  before the port).
