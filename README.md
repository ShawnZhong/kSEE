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

Start with a **tour** (on the left): a question about the kernel ("What
happens in read()?", "Why does open() fail?", …), answered by a small C
program (user/tours/, plain C on nolibc) that runs as a real process. Run
it with its stops: the kernel pauses at each, with a note on what is
happening, the program's line, the call stack with every call's arguments
(the Stack tab; pointers open into their structs), and the source. Continue
to the next stop, or ▶| / ▶ Play line by line. Afterwards the program's run
is in the history (a lane of its own, a flame chart of its calls): click a
program line for what the kernel did for it.

The kernel is live (● LIVE): it runs on its own and the history grows. The
Playground makes any syscall from a typed form (fds by name, flags,
structs), described as strace would. ⤵ steps into one. Click a line number
in the source for a breakpoint. The kernel's clock stands still while it is
paused (❚❚ PAUSED); ↺ REPLAY is the recorded past.

A tour is one C file: `// tour:`, `// question:`, and `// stop FUNCTION:
NOTE` lines at the top, and `// stops here` on the lines whose syscalls the
stops are for.

`./ksee.sh test --ui` (with `$CHROME` set to a headless Chromium) also checks
the page for exceptions and saves a screenshot, `web/ui.png`. The build is
`ksee build` (crates/ksee-cli/src/build.rs).
