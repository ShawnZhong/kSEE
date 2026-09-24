# kSEE

Interactive, browser-based exploration of the execution of the real Linux
kernel: correlate what Linux is doing with its source code, kernel objects,
semantic events, and causal history.

Linux is compiled directly to WebAssembly through a minimal `arch/wasm`
(wasm32, NOMMU, UP first), with one Web Worker per Linux task sharing a single
kernel `WebAssembly.Memory`.

- [docs/design.md](docs/design.md) — project direction and architecture
- [docs/related/](docs/related/) — survey of existing Linux-on-Wasm systems

`refs/` (gitignored) holds local clones of the related projects for reading.

## Layout

- `linux/`: the kernel with `arch/wasm` (submodule, ShawnZhong/kSEE-linux)
- `plugin/`: the LLVM pass that instruments the kernel (basic blocks, calls)
- `crates/`: Rust: `ksee-core` (trace decoding, views, types), `ksee-runtime`
  (wasm that runs in the kernel's memory and drives the page), `ksee-cli`
  (`./ksee.sh`: build, test, serve; pack the kernel for the page)
- `web/`: the page; the build puts what it loads there too (gitignored)
- `user/`: `/init`, the process the page's syscalls run in; `tests/`: see below

## Running

    ./ksee.sh              # build what changed, serve http://127.0.0.1:8000/
    ./ksee.sh test         # under Deno: syscalls, the debugger, the trace
    ./ksee.sh type task_struct

Make syscalls from the page's notebook: lines like
`fd = openat(AT_FDCWD, "/tmp/hello", O_WRONLY|O_CREAT, 0644)` or
`st = new struct stat`, which `/init` runs (pick an example, or
"+ syscall" for any of the kernel's, with its buffers set up). A line's
result links to what it ran: the functions in order and their source (the
whole file, with the lines that ran and how often). The history at the top
shows each task's syscalls over the trace ring; click or drag to replay.
Click a line number to set a breakpoint: the kernel stops there; step, and
look at the current task, the runqueue, any global or type.

`./ksee.sh test --ui` (with `$CHROME` set to a headless Chromium) also checks
the page for exceptions and saves a screenshot, `web/ui.png`. The build is
`ksee build` (crates/ksee-cli/src/build.rs).
