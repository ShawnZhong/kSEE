# kSEE

kSEE records what the real Linux kernel does, down to every line it runs and
every value it changes, and lets you explore the recording in time, in a
browser. Linux is compiled to WebAssembly through a minimal `arch/wasm`
(one instance on the page's thread; its tasks are call stacks it suspends
and resumes with JSPI) and instrumented by an LLVM pass. It needs a browser
with JSPI: Chrome or Edge 137, Firefox 153, Safari 27, or later.

- [docs/design.md](docs/design.md): the goal, the experience, how it works
  (and [docs/](docs/README.md): the kernel port, the recording, rebuilding
  the past, the runtime and the page)

## Layout

- `linux/`: the kernel with `arch/wasm` (submodule, ShawnZhong/kSEE-linux)
- `plugin/`: the LLVM pass that instruments the kernel (blocks, calls, returns, stores)
- `crates/`: Rust: `ksee-core` (decoding and rebuilding the recording:
  stacks, lines, memory as it was, types), `ksee-runtime` (wasm: boots the
  kernel and runs its tasks, serves the page), `ksee-cli` (`./ksee.sh`: build, test,
  serve; pack the kernel for the page)
- `web/`: the page (HTML, JS, CSS); the build puts it, with what it loads,
  in `target/site/` (served, and deployed)
- `user/`: `init.c` (C on nolibc: it makes the page's syscalls), `examples/`
- `tests/`: the page's tests (see below)

## Running

    ./ksee.sh              # build what changed, serve http://127.0.0.1:8000/
    ./ksee.sh test         # under Deno: syscalls, and what their recordings show
    ./ksee.sh deploy       # build, then push the page to gh-pages (GitHub Pages)
    ./ksee.sh type task_struct

The page boots the kernel and opens the first example (or the one in the
URL, `#ex=read`); the selector above it, and ◀ ▶, switch examples, each on
a fresh kernel (the page starts again), so that each starts from the same
state. An **example** (on the left) is a question about the kernel ("What
happens in read()?", "What is a directory?", …), a program of syscalls that
asks it, and a
tutorial: /init (or a thread or child of it) makes the calls as the
example opens, each recorded, and
the tutorial goes from stop to stop (◀ ▶, or click one): a note on a
function one of the calls runs, the cursor there, the source, and the
Call stack (with each call's arguments and locals, as they
were). ◀ ▶ step, and Play plays, by line (into calls), over calls, or out of
the call: the Call stack's selected call, or the line's; ← →, with Shift over,
with Alt out (F10, F11, Shift+F11 as in a debugger). F4 in the source runs to
the line the cursor is on (its next run; Shift+F4 its last), and [ ] go
from stop to stop.

The program is below the tutorial, a call a line, each written as in C
(`openat(AT_FDCWD, "/mnt/x", O_CREAT | O_WRONLY, 0644)`): click one for its
recording; type in it to change it, and Enter: the page starts a fresh
kernel and makes the calls again, which comes to the very same state
up to the change (the machine is deterministic: its clock is virtual);
the calls are in the URL (`#ex=NAME&p=CALLS&stop=N`: the program if
changed, its calls separated by `;`, and the tutorial's stop), a link to
them. ✕ (or emptying its line) removes a call, ↺ brings the example's
own program back, and an edit left without Enter goes back to what it was. Add a call at the
end. As you type, a list shows what fits (syscalls, the constants for the
parameter, the results so far as `$n`), and the signature. Some examples
have several threads or processes (a futex, a thread's join, a pipe
between two processes): a call can be made by a thread or child of /init
(`19: read(3, [64], 64)`), and a call can block, until a later call wakes
it.

The timeline shows the recording: the call stacks over time, and which
task ran (scroll to zoom, drag to pan; hover a call for what it was and a
preview of where, click it to go there; click a syscall, or double-click
any call, to select it: its records from its start to its end, with
whatever else ran in between). Hover
a name in the source for its value at the cursor, and a value in the Call stack
for who last wrote it; click it to watch it: the Watch follows it as the
cursor moves, with every write to it while it lived (a local: while its
frame did): the member, from what to what, by which line, each a link to
that moment, ◀ ▶ to the write before or after the cursor. The Changes tab
lists what the selected call changed, by object (a struct file's f_pos,
an inode's i_size, a local), in the order they were first written; click
one to watch it. Ctrl+click (or F12) a name for its definition: a
function, a type, a global, a struct's member. Over a function, the source
says who called it in the recording, each caller a link to the call.

An example is one JSON file in user/examples/: a title, a question, and
steps: `{ call, name?, tid?, args, stops: [{ func, note, any? }] }` (`name`:
a name for the call's result, which later arguments may use, as `fd` or
`$2`; `tid`: the runner that makes it, a number or a result's name, `child`
for the one a clone started; else /init), and a `group` (the selector's:
"Start here", "Files, in memory", "Files on ext4", "Time", "Threads and
processes"; the order is `EXAMPLES` in crates/ksee-cli/src/build.rs), the arguments
written as in C: `"AT_FDCWD"`, `"O_CREAT | O_WRONLY"` (the kernel's own
constants, from its UAPI headers), `"0644"`, `"\"a string\""`, `"[64]"`
(a buffer for the kernel to fill), `"{0, 30000000}"` or
`"{.tv_nsec = 30000000}"` (a compound literal, laid out as the parameter's
type says: here a struct timespec), `"{\"init\", NULL}"` (an array of
strings, for a char **: argv), `"fd"` or `"$1"` (a result),
`"mem + 65536"`; see crates/ksee-core/src/args.rs. /init mounts an ext4 disk on /mnt (an
image in the initramfs, through the loop device), with files for the
examples to start from: `/mnt/note.txt`, `/mnt/dir/a.txt`; the root, an
in-memory file system (ramfs: no tmpfs without an MMU), has `/note.txt`.

`./ksee.sh test --ui` (with `$CHROME` set to a headless Chromium) also runs
the page itself (tests/ui/: the read example, a changed program from the
URL, the pipe example), and saves screenshots in `target/`. The build is `ksee build` (crates/ksee-cli/src/build.rs).
