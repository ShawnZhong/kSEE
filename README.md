# kSEE

*Watch the real Linux kernel run, line by line, in your browser.*

kSEE (see the kernel) records what the real Linux kernel does, down to every line it runs and
every value it changes, and lets you explore the recording in time, in a
browser. Linux is compiled to WebAssembly through a minimal `arch/wasm`
(one instance on the page's thread; its tasks are call stacks it suspends
and resumes with JSPI) and instrumented by an LLVM pass. It needs a browser
with JSPI: Chrome or Edge 137, Firefox 153, Safari 27, or later.

- [docs/design.md](docs/design.md): the goal, the experience, how it works
  (and [docs/](docs/README.md): the kernel port, the recording, rebuilding
  the past, the runtime and the page)

## Layout

- `linux.patch`: the port, `arch/wasm` and what it needs elsewhere, as a patch to Linux v7.2.7; `linux/`, the kernel it is built in (the build fetches the stable tree's v7.2.7 there, once, and applies the patch; `ksee patch` writes the patch from it)
- `plugin/`: the LLVM pass that instruments the kernel (blocks, calls, returns, stores, variables, comparisons)
- `crates/`: Rust: `ksee-core` (decoding and rebuilding the recording:
  stacks, lines, memory as it was, types; the views; the programs'
  language), `ksee-runtime` (wasm: boots the kernel and runs its tasks: the
  Machine), `ksee-page` (the page, in Dioxus: the same wasm module as the
  runtime, calling the Machine), `ksee-cli` (the `ksee` command: build,
  test, shot, serve, deploy, pack, patch)
- `web/`: `index.html` (the page's frame and style) and `ksee.js` (it loads
  the module and boots the kernel; the page shows meanwhile); the build puts them, with what they
  load, in `target/site/` (served, and deployed)
- `user/`: `init.c` (sets up, starts the program), `prog.c` (the page's program: it makes its syscalls), `syscall.h` (a syscall, as arch/wasm makes one: no C library), `examples/`

## Running

    ./setup.sh             # once: LLVM, GNU make and sed, mkfs.ext4, Rust + wasm target (Homebrew or apt); Chromium in $CHROME for tests
    ./ksee.sh              # build what changed, serve http://127.0.0.1:8000/
    ./ksee.sh test [--ui]  # in Chromium ($CHROME): the kernel's tests; --ui also the page's (screenshots in target/)
    ./ksee.sh deploy       # build, then push the page to gh-pages (GitHub Pages)

macOS: if linking fails with a `tapi … unknown architecture` error, remove
SDKs newer than your Command Line Tools from
/Library/Developer/CommandLineTools/SDKs.

The page boots the kernel and opens the first lesson (or the one in the
URL, `#ex=read`). On the left is the lesson; its head lists every lesson
by chapter (☰ Lessons, those opened before ticked), with ◀ ▶ for the one
before and the next; each opens on a fresh kernel, so that each starts
from the same state.

A lesson is its chapter and place ("Start here · 1 of 2"), its title and
question, the paragraphs that explain it, then its program, a line at a
time, as the page makes it (with more than one runner, in lanes: a column
each, in its task's color; a call that blocked has its lane dotted until
it returns). Each line shows what it did (`= 3`, a call's result, an error
with its name: `-2 (ENOENT)`; a call that blocked until a later line:
`= 6 (during line 7)`), and the variables the call changed under it
(`fds = {3, 4}`). The paragraphs' links lead into the recording: a call, a
function in it (the cursor at its first line; the lesson opens at the
first), what it changed; a try runs another program (↺ reset: the lesson's
own again).
Every line is written in place: as you type, a list of what fits there
(syscalls, the constants that fit the parameter, the names of the lines
before), and the call's signature with the argument at the cursor in
bold; Enter makes the program again (a line emptied is removed; one that
cannot be made says why, and nothing runs). The URL keeps the lesson and,
if changed, the program (`#ex=write&p=…`).

On the right is the recording: on top, an overview of all of it (who
ran when, where each syscall starts, the window shown, the cursor), and
the window as a stack chart, the call stacks over time (the wheel zooms, a
drag pans; "zoom": the call selected, "whole": all of it; hover a bar:
what the call was; a click goes there); the task strip under it (who ran
when, an idle stretch, a worker that names itself); a box to find a
function by any part of its name (its bars light up, Enter goes to the
next). Under them the steps, as a debugger's: ◀ ▶ a line (← →), over the
calls (Shift), out of the function (Alt); ▲ ▼ the frame (↑ ↓), and the
cursor's place. Then the source at the cursor (the whole file, scrolling;
the function's lines edged, those that ran in the call green, a line's
number: how often, a click runs to its next run; what a line stored at its
end; a name's value on hover, Ctrl+click: its definition; the cursor's
line with its values under it), and the tabs: the Stack (the calls then,
innermost first, the one chosen open, its arguments and locals as memory
was then, a value opening into its members or what it points to, hovered:
who last wrote it), Tasks (every task then, as a tree, what each is doing,
its task_struct to open), Objects (what the call changed: each object,
under the one that points to it, each member a function changed in turn,
from what to what, a click: that write), Console (the kernel's log and the
programs' output), Trace (the raw records). The splits between them drag
(the sizes are kept).

An example is one JSON file in user/examples/: a title, a question, `text`
(the explanation: paragraphs, with links into the recording as
`[label](2:vfs_read)`, the first call of vfs_read in call 2's recording,
`:any` for one on any task, or `[label](2)`, call 2 itself, and a tab to
show, `:stack`, `:objects`, `:tasks` or `:console`; `code` in
backticks; `[label](try:1)` runs the example's `tries[0]`, another program
written as lines separated by `;`), `steps`: the program, its lines as the
page writes them out (`"int fd = openat(AT_FDCWD, \"/note.txt\", O_RDONLY)"`,
`"char buf[64]"`, `"child: dup3(fds[0], 0, 0)"`: a call, its result named
and typed as C declares it, or a variable declared; `child:` the runner
that makes it, a thread id or a result's name, else the program); its
chapter and place in the course are `EXAMPLES` in
crates/ksee-cli/src/build.rs ("Start here", "A process", "Files, in
memory", "Files on a disk (ext4)", "Waiting and waking", "Requests in memory
(io_uring)", "Your own"). A call's arguments are written as in C:
`AT_FDCWD`, `O_CREAT | O_WRONLY` (the kernel's own constants, from its UAPI
headers), `0644`, `"a string"`, `{0, 30000000}` (a compound literal, laid
out as the parameter's type says: here a struct timespec), `{"init", NULL}`
(an array of strings, for a char **: argv), `fd` (a result, declared:
`int fd = openat(…)`), `buf` (a variable: its address), `fds[1]`,
`rings->sq.tail` (a line can assign to one: `rings->sq.tail = 1`); see
crates/ksee-core/src/program/. /init mounts an ext4 disk on /mnt (an
image in the initramfs, through the loop device), with files for the
examples to start from: `/mnt/note.txt`, `/mnt/dir/a.txt`; the root, an
in-memory file system (ramfs: no tmpfs without an MMU), has `/note.txt`.

`./ksee.sh test` runs the tests in headless Chromium (`$CHROME`): the
kernel's (every example's recordings: stacks, lines, memory as it was, who
wrote a value, tasks, the links); `--ui` also the page's (read, a changed
program from the URL, pipe, a phone's screen), a screenshot each in
`target/`. `ksee shot` looks at the page as it is built (after a test, or
at a hash). See [docs/runtime.md](docs/runtime.md).
