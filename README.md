# kSEE

kSEE records what the real Linux kernel does, down to every line it runs and
every value it changes, and lets you explore the recording in time, in a
browser. Linux is compiled to WebAssembly through a minimal `arch/wasm`
(one Web Worker per task, one shared kernel memory) and instrumented by an
LLVM pass.

- [docs/design.md](docs/design.md): the goal, the experience, how it works
  (and [docs/](docs/README.md): the kernel port, the recording, rebuilding
  the past, the runtime and the page)

## Layout

- `linux/`: the kernel with `arch/wasm` (submodule, ShawnZhong/kSEE-linux)
- `plugin/`: the LLVM pass that instruments the kernel (blocks, calls, returns, stores)
- `crates/`: Rust: `ksee-core` (decoding and rebuilding the recording:
  stacks, lines, memory as it was, types), `ksee-runtime` (wasm: boots the
  kernel in Workers, serves the page), `ksee-cli` (`./ksee.sh`: build, test,
  serve; pack the kernel for the page)
- `web/`: the page; the build puts what it loads there too (gitignored)
- `user/`: `/init`, `examples/`, `tests/` (C on nolibc)
- `tests/`: the page's tests (see below)

## Running

    ./ksee.sh              # build what changed, serve http://127.0.0.1:8000/
    ./ksee.sh test         # under Deno: programs, and what their recordings show
    ./ksee.sh type task_struct

The page boots the kernel and runs the first example. An **example** (on
the left) is a question about the kernel ("What happens in read()?", "Why
does open() fail?", "ext4: what happens when a file is deleted?", …) and a
small C program that asks it (user/examples/, plain C on nolibc): opening
one runs it in the kernel, recorded, to its end, and goes to its first
stop: its note, the program's
line, the source, and the Stack (the call stack, with each call's
arguments and locals, as they were). Next stop, ◀ / ▶| line by line, or
Play.

The timeline shows the recording: the call stacks over time, and which
task ran (scroll to zoom, drag to pan; hover a call for what it was and a
preview of where, click it to go there, double-click to zoom to it). Hover
a name in the source for its value at the cursor; right-click to watch it;
✎ on a value says which code last wrote it. ● Record records the kernel
until you stop it.

An example is one file in user/examples/: `// example:`, `// question:`,
and `// stop FUNCTION: NOTE` lines at the top, and `// stops here` on the
lines whose syscalls the stops are for. /init mounts an ext4 disk on /mnt
(an image in the initramfs, through the loop device).

`./ksee.sh test --ui` (with `$CHROME` set to a headless Chromium) also runs
the page itself through two examples, and saves a screenshot,
`web/ui.png`. The build is `ksee build` (crates/ksee-cli/src/build.rs).
