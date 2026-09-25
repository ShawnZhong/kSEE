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
- `user/`: `init.c` (C on nolibc: it makes the page's syscalls), `examples/`
- `tests/`: the page's tests (see below)

## Running

    ./ksee.sh              # build what changed, serve http://127.0.0.1:8000/
    ./ksee.sh test         # under Deno: syscalls, and what their recordings show
    ./ksee.sh type task_struct

The page boots the kernel and opens the first example. An **example** (on
the left) is a question about the kernel ("What happens in read()?", "The
life of a file on ext4", …) and a few syscalls that ask it: /init makes
them for the page, one at a time (▶ Next call), each recorded, and the
page goes to the call's first stop: its note, the source, and the Stack
(the call stack, with each call's arguments and locals, as they were).
Next stop, ◀ / ▶| line by line, or Play. Add your own call below the list:
write it as in C, `openat(AT_FDCWD, "/mnt/x", O_CREAT | O_WRONLY, 0644)`,
and Enter (Tab completes a syscall's name; the hint shows its signature).
✎ on a call made goes back to before it, to make another instead: the page
starts a fresh kernel and makes the calls before it again (a kernel's
tasks' stacks are its Workers', so it cannot be put back as it was); the
calls are in the URL (`#back=`), a link to them.

The timeline shows the recording: the call stacks over time, and which
task ran (scroll to zoom, drag to pan; hover a call for what it was and a
preview of where, click it to go there; click a syscall, or double-click
any call, to select it: its records from its start to its end, with
whatever else ran in between). Hover
a name in the source for its value at the cursor;
✎ on a value says which code last wrote it. ● Record records the kernel
until you stop it.

An example is one JSON file in user/examples/: a title, a question, and
steps: `{ call, args, stops: [{ func, note, any? }] }`, the arguments
written as in C: `"AT_FDCWD"`, `"O_CREAT | O_WRONLY"` (the kernel's own
constants, from its UAPI headers), `"0644"`, `"\"a string\""`, `"[64]"`
(a buffer for the kernel to fill), `"{0, 30000000}"` (64-bit values),
`"$1"` (call 1's result); see web/args.js. /init mounts an ext4 disk on /mnt (an
image in the initramfs, through the loop device).

`./ksee.sh test --ui` (with `$CHROME` set to a headless Chromium) also runs
the page itself through two examples, and saves a screenshot,
`web/ui.png`. The build is `ksee build` (crates/ksee-cli/src/build.rs).
