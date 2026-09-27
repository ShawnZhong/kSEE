# Rebuilding the past

The host rebuilds everything the page shows from the records alone
(crates/ksee-core: `recording.rs`, `stack.rs`, `mem.rs`, `syscalls.rs`),
with no help from the kernel. Each snapshot decodes the ring into a
`Recording`; `view.rs` turns it into what the page shows.

## Which task ran

Records do not say which task wrote them. The running task changes at
known places, so a pass over the records tracks it:

- a call of `__switch_to(prev, next)` notes `next`; the task changes when
  that call **returns** (on the next task's stack);
- a new task starts in `wasm_task_entry(p)`;
- before the first switch, its `prev` ran.

Who a task is (pid, comm) comes from its task_struct, in memory now (so a
task that exited meanwhile, a kernel thread or a runner's child, may show
as whoever has its address now; a runner that execs shows its new comm).

## Calls and stacks

A shadow stack per task (stack.rs). LLVM's entry block has no predecessors,
so **block 0 runs once per call**: it pushes a frame. A block of a function
further down the stack is a return into it (the frames above pop); a return
record pops its function's frame. A block of a function not on the stack
(its call was before the recording began) restarts the stack from it. A
task starts (`wasm_task_entry`) with an empty stack: a task that exited
never returned from its last calls, and a new one may get its address.

A frame's arguments and its frame base are the Args record after its
block 0; its locals are at their offsets from the base (`Types::locals`).
One walk (`Walk`)
serves the Stack at a record (`stack_at`), the stack chart (`chart`: every
task's stack at the columns' first records) and line steps (`lines`): each
call with the line it is at.

## Lines within blocks

A block runs several source lines, and calls functions in between. The
plugin's `L` lines give a block's lines in stretches split at its calls;
`lines` walks the records per task: at a block, its first stretch; after a
return into it, the next. So stepping goes line by line, into and out of
calls, including lines inlined from other functions. Each line step says
which of its record's lines it is (`nth`: the Stack there) and how deep it
runs (calls deep, and inlined deep): the level stepping over or out of a
call compares. `run_of` finds the next (or last) record whose block runs a
given line: "run to a line".

## Inlined code

The optimizer inlines many small functions (`fdget_pos`, `current`, the
`SYSCALL_DEFINE` wrappers), so there is no call record for them, but the
line tables say which code is inlined from where (its inline chain). A
call's position is its last line; if that line is inlined, each function of
the chain is a frame of its own on the stack (no arguments: there was no
call), at the line inside it, and the real call is at the line where the
chain starts. The stack chart shows them as calls too, and the line a
record is at names the function its code is inlined from: the page does
not tell inlined code from a call (only that it has no arguments). The
one exception is the machine's own primitives, `arch/wasm/include/asm`'s
and `include/asm-generic`'s `current`, interrupt flag, atomics and bit
ops (arch::PRIMITIVES): inlined
everywhere, they are the machine, not calls, so their code counts as the
line it is inlined at (Tables::fold_primitives).

## Syscalls

A syscall is a call of `wasm_syscall` (its arguments: the number and the
syscall's own) up to its return (the result), on one task. On the Stack,
the syscall's entry (`wasm_syscall` and the `SYSCALL_DEFINE` wrappers) is
one frame, named for the syscall, its arguments shown as the kernel's
constants (`AT_FDCWD`, `O_CREAT | O_WRONLY`).

## Memory as it was

Every store has the old value. `mem::History` indexes them by address.
Memory as it was after record `i` is memory at the end of the recording
with **every later store undone**, byte by byte: the oldest later store
that covers a byte has its value (`PastMemory`). A call's recording ends
only when the kernel is quiet (idle, waiting for the page), and nothing
runs until the next call: so memory now is memory as the recording ends.

The same index answers "who last wrote this?": the newest store to a range
at or before the cursor, and the line it was on (where `lines` says the
task was then); an object's history (`writes`): every store to a value
of a type at an address in a span of records, its lifetime (the page
gives a local's frame, from its call to its return: the slot is another
call's before and after; heap objects' lifetimes are not known yet, so
theirs is the whole recording), each as the member it wrote (the one its
first byte is in: `Types::leaf`), printed as its type before and after
(memory as it was); and what a span of records changed (`changes`): its
stores grouped by the object they hit, the objects being what the code
there held (a call's pointer argument to a struct, a local, the task
running: the objects the code named; what it did not, the heap reached
through locals and bulk copies, is counted as "other"). An object is said
as the kernel's own fields name it (`describe`): a task by comm and pid,
a file by the fd pointing at it in the task's table and its dentry's
name, an inode by number and filesystem, a dentry, a path, a kiocb by
the file's, a super_block by its type and id.

What it cannot show: values only in wasm locals (registers, in effect),
and old contents of bulk writes beyond 256 bytes (their record says
where and how much, not what: going back shows such bytes as they are
now).

## Going back

A kernel cannot be put back as it was (its tasks' stacks are the
browser's), but it can be run again: the machine is deterministic (see
[runtime.md](runtime.md)), so a fresh kernel given the same calls comes to
the same state, record for record; then another call can be made instead.

## Records keep their names

The ring goes on while the page looks at a snapshot, so indexes shift from
one snapshot to the next. Each record's **id** counts the pairs ever
written up to it: the same record has the same id in every snapshot, so a
selection (a syscall, a call's run) is found again.
