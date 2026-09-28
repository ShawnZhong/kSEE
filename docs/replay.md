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
The stacks are rebuilt once per snapshot, in one pass (`Calls`): every
call (its function, first record, the record that ends it, the call it
was made from and where that one was then), and per record where its
task is after it (its innermost call, the block and the stretch of its
lines). So the Stack at a record (`stack_at`: from the innermost call out,
through each call's caller), where a call ends (`call_end`), the stack
chart (`chart`: every task's stack at the columns' first records; the
columns are equal in steps, the recording's unit of time: a block run, so
a store takes none and a memset is not longer than a line) and a store's
line are lookups, not walks from the first record.

## Lines within blocks

A block runs several source lines, and calls functions in between. The
plugin's `L` lines give a block's lines in stretches split at its calls;
at a block, its first stretch runs; after a return into it, the next
(`lines`: those of a span of records). So stepping goes line by line, into and out of
calls, including lines inlined from other functions. Each line step says
which of its record's lines it is (`nth`: the Stack there), where it is
`at` (the record of its first store, which says its line: the cursor
goes there, and memory as it was; a line with no store, or a stretch's
first, is at the block's record) and how deep it runs (calls deep, and
inlined deep): the level stepping over or out of a call compares. A
store's line is the one it says, in the block its task is in
(`line_at`). `run_of` finds the next (or last) record whose block runs a
given line: "run to a line".

## Inlined code

The optimizer inlines many small functions (`fdget_pos`, `current`, the
`SYSCALL_DEFINE` wrappers), so there is no call record for them, but the
line tables say which code is inlined from where (its inline chain). A
call's position is its last line; if that line is inlined, each function of
the chain is a frame of its own on the stack (no arguments: there was no
call), at the line inside it, and the real call is at the line where the
chain starts. The stack chart shows them as calls too, and the line a
record is at names the function its code is inlined from. The page says
which is which (an inlined frame is marked so on the Stack, italic in
the Calls) and shows no arguments for inlined code: there was no call. The
one exception is the machine's own primitives, `arch/wasm/include/asm`'s
and `include/asm-generic`'s `current`, interrupt flag, atomics and bit
ops (arch::PRIMITIVES): inlined
everywhere, they are the machine, not calls, so their code counts as the
line it is inlined at (Tables::fold_primitives).

## Syscalls

A syscall is a call of `wasm_syscall` (its arguments: the number and the
syscall's own) up to its return (the result), on one task. Its frame on
the Stack says which syscall, its arguments typed as the syscall's
parameters and shown as the kernel's constants (`AT_FDCWD`, `O_CREAT |
O_WRONLY`). The frames inside it (`__wasm_sys_openat`, the
`SYSCALL_DEFINE` shims inlined into it) show as they are, nothing folded;
the wrapper's parameters are the syscall's, as its `SYSCALL_DEFINE` line
has them (arch::SYSCALL_WRAPPER; its own DWARF parameter is `regs`).

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
(memory as it was); and the stores of a span of records (`stores`), in
order, each named as the code names what it hit (a call's pointer
argument to a struct, a local and what a pointer local points to as the
call ends, the task running and what its pointer members point to, a
global: "vfs_read's file->f_pos", "current->files->next_fd"; what the
code did not name, by its address).

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
