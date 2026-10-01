# kSEE docs

- [design.md](design.md): the goal, the experience, how it works, limits.

How it works, part by part:

- [kernel.md](kernel.md): Linux on wasm (arch/wasm): tasks as suspended stacks (JSPI),
  interrupts and the virtual clock, programs in kernel memory, threads and
  vfork, /init, the program and the runners that make the page's syscalls, a disk, the
  console.
- [recording.md](recording.md): what the compiler records, the ring, and
  what the kernel says about itself.
- [replay.md](replay.md): rebuilding the past from the records: tasks,
  stacks, lines and stepping, syscalls, memory as it was, going back.
- [runtime.md](runtime.md): the runtime (one wasm module in the kernel's
  memory), the page, the tests.

The build and the commands:

- `ksee.sh` builds the `ksee` command (crates/ksee-cli), then runs it:
  `ksee` builds and serves the page; `ksee build` builds (the kernel, the
  runtime and page, the programs, into target/site/);
  `./setup.sh` installs what it needs (LLVM, GNU make and sed, mkfs.ext4,
  Rust + its wasm target; Chromium in $CHROME for the tests).
- `ksee test [--ui]`: the kernel's tests, the page's too with `--ui`
  ([runtime.md](runtime.md): what each checks); `ksee patch` writes linux.patch from linux/
  ([kernel.md](kernel.md): the port); the build's pack prepares the kernel for
  the page ([recording.md](recording.md): what it adds).

Background:

- [ideas.md](ideas.md): what was proposed and is still open, each a line
- [research.md](research.md): directions surveyed and not built
  (counterfactual execution, ext4 feasibility numbers), so they are not
  re-done.
