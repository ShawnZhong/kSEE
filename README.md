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

The kernel is live (● LIVE): it runs on its own, and the history at the top
grows (a lane per task, its syscalls as bars). Make syscalls with the cards
on the left: pick any of the kernel's (or a scenario), fill in its typed
form (fds by name, flags, structs), and `/init` makes it; the card then
reads like strace. Its result links to what it ran: the flame chart of its
calls, rebuilt from the trace, and the source at the cursor.

To go through a syscall line by line, press ⤵ (Step into) on its card: the
kernel stops where the syscall starts (❚❚ PAUSED), with the call stack and
each call's arguments (→ opens a pointer's object). ▶| runs a line; ▶ Play
runs line after line, at the speed picked. Or replay one already made
(↺ REPLAY): select it and Play, or drag the cursor through the history.
Click a line number in the source to set a breakpoint. The kernel's clock
stands still while it is paused.

`./ksee.sh test --ui` (with `$CHROME` set to a headless Chromium) also checks
the page for exceptions and saves a screenshot, `web/ui.png`. The build is
`ksee build` (crates/ksee-cli/src/build.rs).
