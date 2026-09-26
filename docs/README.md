# kSEE docs

- [design.md](design.md): the goal, the experience, how it works, limits.

How it works, part by part:

- [kernel.md](kernel.md): Linux on wasm (arch/wasm): tasks as suspended stacks (JSPI),
  interrupts and the virtual clock, programs in kernel memory, threads and
  vfork, /init and the runners that make the page's syscalls, a disk, the
  console.
- [recording.md](recording.md): what the compiler records, the ring, and
  what the kernel says about itself.
- [replay.md](replay.md): rebuilding the past from the records: tasks,
  stacks, lines and stepping, syscalls, memory as it was, going back.
- [runtime.md](runtime.md): the runtime (one wasm module in the kernel's
  memory), the page, the tests.

Background:

- [related/](related/): other Linux-on-Wasm systems (research notes from
  before the port).
