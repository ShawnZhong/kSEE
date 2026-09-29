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
- `plugin/`: the LLVM pass that instruments the kernel (blocks, calls, returns, stores, variables, comparisons)
- `crates/`: Rust: `ksee-core` (decoding and rebuilding the recording:
  stacks, lines, memory as it was, types), `ksee-runtime` (wasm: boots the
  kernel and runs its tasks, serves the page), `ksee-cli` (`./ksee.sh`: build, test,
  serve; pack the kernel for the page)
- `web/`: the page (HTML, JS, CSS); the build puts it, with what it loads,
  in `target/site/` (served, and deployed)
- `user/`: `init.c` (sets up, starts the program), `prog.c` (the page's program: it makes its syscalls; C on nolibc), `examples/`
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
asks it, and an explanation: the program (or a thread or child of it) makes
the calls as the example opens, each recorded, and the explanation's
links lead into the recording: a function one of the calls runs (the
cursor at its call: the source, and the Stack with each call's
arguments and locals, as they were), or a call. Two pairs of keys
explore it: ← → step in time, to the next line run at the level (that
deep or shallower: what the calls there make is stepped over; the
caller's lines, and the siblings' at the level, are not; with no level,
at first, every line; past the syscall's end, or before its start, into
its task's next syscall, or the one before); ↑ ↓ move the level, a row
of the Stack up or down (the callee, the caller; ↓ from every line: the
innermost; ↑ past the innermost: every line again, "all"), as a click on a
call sets it (in the timeline, on the Stack); the level is shown by the
buttons as its row, "level 4", with "all" to leave it. ◀ ▶ ▲ ▼ under the timeline are the four keys. F4 in the
source runs to the line the cursor is on (its next run; Shift+F4 its
last).

The program is below the explanation, a call a line, each written as in C
(`openat(AT_FDCWD, "/mnt/x", O_CREAT | O_WRONLY, 0644)`): click one for its
recording; type in it to change it, and Enter: the page starts a fresh
kernel and makes the calls again, which comes to the very same state
up to the change (the machine is deterministic: its clock is virtual);
the calls are in the URL (`#ex=NAME&p=CALLS`: the program if
changed, its calls separated by `;`), a link to them. ✕ (or emptying its line) removes a call, ↺ reset (always there) brings the example's
own program back, and an edit left without Enter goes back to what it was. Add a call at the
end. As you type, a list shows what fits (syscalls, the constants for the
parameter, the results so far as `$n`), and the signature. Some examples
have several threads or processes (a futex, a thread's join, a pipe
between two processes): a call can be made by a thread or child of the program, and a call can
block, until a later call wakes it. With more than one task, the program
is in lanes: a column per task (its tid over it, in the timeline's task
colors), the lines in the order they were made, down being time; a call
that blocked has its lane marked until the line during which it returned;
each lane has its own line to add a call, made by its task (written out,
as in the URL: `child: read(0, buf, 64)`, `20: …`). A line can also declare a variable of the program's, as in C: `char
buf[64]`, `int fds[2]`, `struct __kernel_timespec ts = {0, 30000000}`,
memory of its own in the program's process; its name then stands for its
address (`read(fd, buf, 64)`, `&ts`), `fds[1]` for an element as it is
when the call is made, and each call's line says the variables it changed,
as C writes them (`fds = {3, 4}`, `buf = "hello from memory\n"`). The same calls can be made from JavaScript, in the browser's console:
`await ksee.kernel.sys.read(fd, buf, 64)` (web/kernel.js: JS values as
C's, a struct as an object; a call that blocks stays pending while others
go on), each recorded as the page's are.

The timeline shows the recording: the call stacks over time (its unit is
a step, a basic block run: a store takes no room), and which
task ran; the whole recording at first (scroll to zoom, drag to pan;
"zoom" or z: the window at the call selected, else the syscall, "whole" or x: all
of it again; hover a call for what it was (its arguments, what it returned: `ksys_read = 26`): the
source, the Stack and the Objects show that moment while the pointer is there; click it to fix the cursor there, and select it (on the Stack; a syscall: selected too,
its records from its entry to its exit, with whatever else ran in
between, the lines to step through). The
cursor's line carries its values as it is about to run (as a debugger
stops before a line: memory and the variables as the lines before it left
them), and each line what it
stored or set so far in the syscall selected, each thing it wrote its last
change and how many times (`f_pos: 0 → 26 ×2`, a
variable's `last_pos: (uninitialized) → -1`; a struct copied at once,
each member: `mnt: NULL → 0x1c07910   dentry: NULL → 0x1c11dd00`; a name
once, as the text has it, though two inlined copies' `sb` wrote it: `sb:
NULL → 0x1c49000, (uninitialized) → 0x1c49000`): click it for its last write; not the
temporaries the kernel's macros make up, `__UNIQUE_ID_…`); a step that lands after a call says what it returned.
Hover
a name in the source for its value at the cursor (a name with one is
underlined, dotted: the arguments and locals of the calls on the Stack; a
struct's value opens, field by field, on a click on its name), and a value in the Stack
for who last wrote it. The Stack shows the selected call's
values (the innermost's, unless another is selected: a click on it, ▲ ▼,
a bar in the timeline), the others a row each, as a debugger's variables
are the selected frame's (every call's, on asking); its locals are all
of them, in the order it declares them, those in memory and those the
code keeps in wasm locals, as the recording has them set. The Tasks tab lists every task the
recording has, at the cursor, as a tree (a thread under its process's
leader, a process under its parent): the one running marked, each where it is
(`prog(20)  in read`: its syscall, else its outermost call), its name a
link to its last moment (the Stack then shows its stack), its task_struct
as it was on asking; the Stack's `current task` is a link to it. The
Objects tab says what the syscall selected did to the kernel's data, all
of it, always: its head says it in a line (`read by prog(19) changed
3 objects: file, folio, inode`), then a box per object of the kernel's
data it changed (as the code
declares it: `struct file *file in vfs_read`, `struct files_struct
*prog(19)->files`, `unsigned long nr_inodes`, and by its
address; a global, with where it is declared, a link to it; not a
stack's: a call's locals are the Stack's; nor the machine's plumbing),
first those it left changed, then those it left as they
were (a lock taken and released, a count taken and dropped); in a box, the
members it left changed first, each member it changed,
a line a function that changed it in turn, from what to what (`f_mode
0 → 0x4000001  init_file`, then `0x4000001 → 0x40a801d  do_dentry_open`;
a pointer, to another object: that one, `fd_array[3]  NULL → f in
create_pipe_files`, a link to it; a count one took and another dropped,
dim); a line goes to its write. As a tree: each object under
the first that points to it (`← f_inode`), the one holding the task
first; the head names them, once each (`folio ×23`). The moment shown (the
pointer's over the timeline, else the cursor's) only colors it: the
writes not made by then are gray, those it is among marked. One store may change several members (a struct
copied at once: `f_path.mnt` and `f_path.dentry`): each is a change of
its own. The line the cursor is on shows its values at
its end, as a debugger does (`fd = 3   count = 64`). Objects are said as
the kernel's own fields name them: a file by its fd and name, an inode by
its number and filesystem, a task by its comm and pid. The Trace tab is
the raw trace, a line a record (a call with its arguments, a block, a
store from what to what, a return): what every other view is built from.
Ctrl+click (or F12) a name for its definition: a function, a type, a
global, a struct's member.

An example is one JSON file in user/examples/: a title, a question, `text`
(the explanation: paragraphs, with links into the recording as
`[label](2:vfs_read)`, the first call of vfs_read in call 2's recording,
`:any` for one on any task, or `[label](2)`, call 2 itself, and a tab to
show, `:stack`, `:tasks` or `:trace`, or `:objects`: the
function's call selected, at its return, what it changed; `code` in
backticks; `[label](try:1)` runs the example's `tries[0]`, another program
written as lines separated by `;`, to see what changes), `steps`: the
program, its lines as the page writes them out (`"int fd = openat(AT_FDCWD,
\"/note.txt\", O_RDONLY)"`, `"char buf[64]"`, `"child: dup3(fds[0], 0,
0)"`: a call, its result named and typed as C declares it, or a variable
declared; `child:` the runner that makes it, a thread id or a result's
name, else the program), and a `group` (the selector's: "Start here", "Files,
in memory", "Files on ext4", "Time", "Threads and processes"; the order is
`EXAMPLES` in crates/ksee-cli/src/build.rs). A call's arguments are
written as in C: `AT_FDCWD`, `O_CREAT | O_WRONLY` (the kernel's own
constants, from its UAPI headers), `0644`, `"a string"`, `{0, 30000000}`
or `{.tv_nsec = 30000000}` (a compound literal, laid out as the
parameter's type says: here a struct timespec), `{"init", NULL}` (an
array of strings, for a char **: argv), `fd` or `$1` (a result), `buf` (a
variable: its address), `fds[1]`, `mem + 65536`; see
crates/ksee-core/src/program/ (Machine.statement parses a line). /init mounts an ext4 disk on /mnt (an
image in the initramfs, through the loop device), with files for the
examples to start from: `/mnt/note.txt`, `/mnt/dir/a.txt`; the root, an
in-memory file system (ramfs: no tmpfs without an MMU), has `/note.txt`.

`./ksee.sh test --ui` (with `$CHROME` set to a headless Chromium) also runs
the page itself (tests/ui/: the read example, a changed program from the
URL, the pipe example), and saves screenshots in `target/`. The build is `ksee build` (crates/ksee-cli/src/build.rs).
