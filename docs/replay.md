# Rebuilding the past

The host rebuilds what the page shows from trace records, packed kernel
metadata, and memory at the end of the snapshot (`ksee-core`:
`recording.rs`, `stack.rs`, `mem.rs`). Each snapshot decodes the ring into a
`Recording`; `view/` turns it into named page data (`frames.rs`: lines and the
Stack; `stores.rs`: named stores, notes, and writers; `objects.rs`: what a
call changed; `value.rs`: typed values).

The live kernel does not answer replay queries. It supplies the final memory
image and the metadata packed into its WebAssembly module; core applies the
recorded history to those inputs.

## Which task ran

The first record of a requested syscall is a runtime-generated `Task` record
containing the running `task_struct` address. A pass over the later records
then tracks changes in order:

- a `Task` record sets the running task directly and clears any pending switch;
- a call of `__switch_to(prev, next)` notes `next`, and the task changes when
  that call **returns** on the next task's stack;
- a new task starts in `task_entry(p)` (its `__wasm_export` name); and
- if an older or truncated recording begins before an identifying record, the
  `prev` argument of its first observed switch fills the unknown prefix.

Who a task was (pid, comm) comes from its task_struct as memory was at
the record it is named at: a task renamed later is named as it was then.

The tasks at a record: the kernel's task list as memory then had it (from
`init_task`, each process by its `tasks` link, each one's threads by its
signal's `thread_head`: arch.rs), then the others the recording has; each
running (on the CPU there), runnable (on the run queue, `on_rq`), sleeping
(off it), exited (no longer on the list) or later (not made yet).

## Calls and stacks

A shadow stack per task (stack.rs). LLVM's entry block has no
predecessors, so **block 0 runs once per call**: it pushes a frame. A
block of a function further down the stack is a return into it (the
frames above pop); a return record pops its function's frame. A block of
a function not on the stack (its call was before the recording began)
restarts the stack from it. A task starts with an empty stack: a task
that exited never returned from its last calls, and a new one may get its
address.

A frame's arguments and its frame base are the words after its block 0
(by the call's record; a wide argument's two joined); its locals in memory
are at their offsets from the base, and those it keeps in wasm locals,
and a parameter it changed, are as its last var record in the call says
(each call's, indexed in the same pass), and so are those of the code
inlined into it (by the inlined function's name: of two inlined copies of
one function in a call, the later's values).
The stacks are rebuilt once per snapshot, in one pass: every call (its
function, first record, the record that ends it, the call it was made
from and where that one was then), and per record where its task is after
it (its innermost call, the block and the stretch of its lines). So the
Stack at a record (from the innermost call out, through each call's
caller), where a call ends, the timeline (at each step, the task running there and its stack) and a store's line are
lookups, not walks from the first record.

## Lines within blocks

A block runs several source lines, and calls functions in between. The
plugin's `L` lines give a block's lines in stretches split at its calls
(`|` where it calls); at a block, its first stretch runs; after a return
into it, the next. So stepping goes line by line, into and out of calls,
including lines inlined from other functions. Each line step says which
of its record's lines it is, where it is `at` (the record just before it
runs, as a debugger stops before a line: before the first record on it or
a later line of its stretch, a store, a comparison or a variable set, each
saying its line; with none, after the stretch's last; a stretch's first
line at the block's record: the cursor goes there, and memory and the
variables are as they were) and how deep it runs (calls deep, and inlined
deep): the depth a step in one function compares. A line that made a
call says what the call returned (`called`), from its return on:
`ret = f(...)` shows the value `ret` got. A store's line is the one it
says, in the block its task is in.

## Records and line steps

These are related but not interchangeable units:

- A **raw record** is one decoded ring event: a block, task, return, store,
  comparison, or variable update.
- A **line step** is reconstructed from a block's ordered source lines and the
  calls between its stretches. One block record can therefore yield several
  steps, while store and comparison records refine positions within them.
The timeline's axis is the line steps themselves (`Recording::steps`): each
syscall's lines exactly as stepping walks them (`View::lines`), and between
syscalls each task's. `View::timeline` finds, once a snapshot, each row's
bars as the exact steps they span (a call, or code inlined into it, for as
long as it is on the stack there) and which task ran when; nothing is
sampled. A window is fractional steps, its start and span: drawing it is a
transform of those spans (a bar thinner than a pixel, once per pixel), and a
hover or click maps a pixel back to its step the same way. So a call's bar is
as wide as the steps it takes, and one step of the cursor moves it one step
across the chart.

## Inlined code

The optimizer inlines many small functions (`fdget_pos`, `current`, the
`SYSCALL_DEFINE` wrappers), so there is no call record for them, but the
line tables say which code is inlined from where (its inline chain). A
call's position is its last line; if that line is inlined, each function
of the chain is a frame of its own on the stack (no arguments: there was
no call), at the line inside it, and the real call is at the line where
the chain starts. The page says which is which (an inlined frame is marked
"inlined" on the Stack) and shows no arguments for inlined code. The one
exception is the machine's own primitives, `arch/wasm/include/asm`'s and
`include/asm-generic`'s `current`, interrupt flag, atomics and bit ops
(arch::PRIMITIVES): inlined everywhere, they are the machine, not calls,
so their code counts as the line it is inlined at.

## Syscalls

A syscall is a call of `wasm_syscall` (its arguments: the number and the
syscall's own) up to its return (the result), on one task. Its frame on
the Stack says which syscall, its arguments typed as the syscall's
parameters and shown as the kernel's constants (`AT_FDCWD`, `O_CREAT |
O_WRONLY`). The frames inside it (`__wasm_sys_openat`, the
`SYSCALL_DEFINE` bodies inlined into it) show as they are, nothing folded;
the body's parameters are the syscall's, as its `SYSCALL_DEFINE` line has
them (its own DWARF parameter is `regs`).

## Memory as it was

Every store has the old value. `mem::History` indexes them by address.
Memory as it was after record `i` is memory at the end of the recording
with **every later store undone**, byte by byte: the oldest later store
that covers a byte has its value. A call's recording ends only when the
kernel is quiet (idle, waiting for the page), and nothing runs until the
next call: so memory now is memory as the recording ends.

The same index answers "who last wrote this?": the newest store to a
range at or before the cursor, and the line it was on; and the stores of
a span of records, in order, each by the region it hit, named as the code
names it (a call's pointer argument to a struct, a local and what a
pointer local points to as the call ends, the task running and what its
pointer members point to, a global: "file->f_pos in vfs_read",
"prog(19)->files->next_fd"; what the code did not name, by its address: a
callee's stack bytes no variable of its names, a register spilled). A
call's local, and an object a pointer reaches in a frame on the stack, are
theirs only while that call runs (the slot is another's before and after).
Named once per span, a store is printed only where it is shown: as the
member it wrote, as its type before and after (memory as it was). What a
call did is its stores by object, each to the outermost object holding it,
the kernel's data, not a stack's (a call's locals, and what a pointer
reaches on a stack, are the Stack's): each member from what it was as the
span began to what it is as it ends, and how: a run a function writing it
in turn, from what to what (one that left it as it was, not; a member no
run changed, not); a pointer, the object it points to as it ends; and each
object under the first that points to it then, and of its task: the one
that made its first write (`Node::by`: another than the span's, one that
ran while it waited, its objects after the span's own).

A store with no line of its own (the compiler gave it none, or one past
its stretch) is on the line its stretch ends on.

## Where a value came from, and why a branch went

A store that copies says where its bytes come from (each store of a
memcpy its own). So a value's way through memory can be followed back
(`origin`): its last write, and while that was a copy, the last write of
what it copied, and so on: the bytes a read returns, from the page cache,
which a write filled from its caller's buffer. It ends where a value was
computed (in wasm locals), or at memory written before the recording.

A comparison that decides a branch has a Compare record: its operands and
its predicate. A line step says the comparisons it made (`tests`: "18 >=
0 ✓"): the values the branch went by, which live only in wasm locals
otherwise. (The comparison as compiled: the optimizer may have turned the
source's around, so it says what was compared, not which way the source's
`if` went.)

What it cannot show: values only in wasm locals (registers, in effect)
that are no named variable's (an intermediate, `a + b` before it is
stored).

## Going back

A kernel cannot be put back as it was (its tasks' stacks are the
browser's), but it can be run again: the machine is deterministic (see
[host.md](host.md)), so a fresh kernel given the same calls comes to
the same state, record for record; then another call can be made instead.

## Records keep their names

The ring goes on while the page looks at a snapshot, so indexes shift from
one snapshot to the next. Each record's **id** counts the words ever
written before it: the same record has the same id in every snapshot, so a
selection (a syscall, a call's run) is found again.
