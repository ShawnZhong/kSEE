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
- `crates/`: Rust: `ksee-core` (trace decoding, views), `ksee-runtime`
  (wasm that runs in the kernel's memory), `ksee-cli` (`./ksee.sh`)
- `web/`: the page, and the few lines of JS that start the runtime
- `user/`: user programs for the initramfs; `tests/`: `make test`

## Running

    make                 # kernel, initramfs, Rust crates, build/vmlinux
    make test            # boot under Deno: shell, debugger, trace views
    ./ksee.sh serve      # then open http://127.0.0.1:8000/web/

In the page, `/init` is a syscall shell (try `help`, `ls /`, `mkdir /tmp`):
each command's syscalls appear on the left; pick one to see the functions it
ran, their source with the lines that ran, and their control flow. Click a
source line number to set a breakpoint: the kernel stops there, and you can
step, look at the current task, globals (`runqueues`), or any type at an
address.

`tools/shot.js` (with `$CHROME` set to a headless Chromium) screenshots the
page, for checking the UI without a browser.
