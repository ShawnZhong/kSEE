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
    ./ksee.sh test         # under Deno: programs, the debugger, the trace
    ./ksee.sh type task_struct

Pick a **tour** (on the left): a question about the kernel ("What happens
in read()?", "Why does open() fail?", …), answered by a small C program
(user/tours/, plain C on nolibc) that runs as a real process, recorded.
The kernel pauses at each of the tour's stops, with a note on what is
happening, the program's line, the source, and the Stack: the call stack
with every call's arguments (pointers open into their structs). Continue to
the next stop, ▶| / ▶ Play line by line, or Finish. Afterwards the
recording is in the history (a lane per task, a flame chart of the calls):
click a program line for what the kernel did for it, drag the cursor back,
and the Stack shows memory as it was then (✎: which code last wrote a
value). Hover a name in the source (an argument, a global, or a field
after them: `file->f_pos`) for its value there; click it to watch it.
● Record records the kernel by hand; click a line number in the
source for a breakpoint. Drag the gray bars to resize the panels.

A tour is one C file: `// tour:`, `// question:`, and `// stop FUNCTION:
NOTE` lines at the top, and `// stops here` on the lines whose syscalls the
stops are for.

`./ksee.sh test --ui` (with `$CHROME` set to a headless Chromium) also checks
the page for exceptions and saves a screenshot, `web/ui.png`. The build is
`ksee build` (crates/ksee-cli/src/build.rs).
