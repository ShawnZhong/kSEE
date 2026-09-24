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
  (`./ksee.sh`: pack the kernel, read dumps, serve the site)
- `web/`: the page: `app.js` wires the panels (`composer.js`, `syscalls.js`,
  `source.js`, `objects.js`, `timeline.js`); `ksee.js` and `worker.js` start
  the runtime
- `user/`: `/init`, the syscall agent; `tests/`: `make test`, `make ui-test`

## Running

    make                 # everything; the site in build/web
    make test            # under Deno: syscalls, the debugger, the trace views
    ./ksee.sh serve      # then open http://127.0.0.1:8000/

Make syscalls from the page (pick one of the kernel's, or a preset): `/init`
makes them. Each appears on the left; pick one to see the functions it ran,
their source (the whole file, with the lines that ran), and their control
flow. Click a source line number to set a breakpoint: the kernel stops there;
step, and look at the current task, the runqueue, any global or type. The
strip at the bottom replays the trace ring: move the cursor to see where the
kernel was.

`make ui-test` (with `$CHROME` set to a headless Chromium) checks the page for
exceptions and saves a screenshot, `build/ui.png`.
