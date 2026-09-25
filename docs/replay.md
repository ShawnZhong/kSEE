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
- a Task record names the task that writes it.

Who a task was (pid, comm) comes from its program's Task record, as long as
it did not exit (`do_exit`) in between; else from memory now. A task_struct
address is reused by later tasks.

## Calls and stacks

A shadow stack per task (stack.rs). LLVM's entry block has no predecessors,
so **block 0 runs once per call**: it pushes a frame. A block of a function
further down the stack is a return into it (the frames above pop); a return
record pops its function's frame. A block of a function not on the stack
(its call was before the recording began) restarts the stack from it. A
task starts (`wasm_task_entry`) with an empty stack: a task that exited
never returned from its last calls, and a new one may get its address.

A frame's arguments are the Args record after its block 0; its locals'
addresses are its Locals record, if it got that far. The stack chart
(`chart`) samples every task's stack at the columns' first records.

## Lines within blocks

A block runs several source lines, and calls functions in between. The
plugin's `L` lines give a block's lines in stretches split at its calls;
`lines` walks the records per task: at a block, its first stretch; after a
return into it, the next. So stepping goes line by line, into and out of
calls, including lines inlined from other functions.

## Inlined code

The optimizer inlines many small functions (`fdget_pos`, `current`, the
`SYSCALL_DEFINE` wrappers), so there is no call record for them, but the
line tables say which code is inlined from where (its inline chain). A
call's position is its last line; if that line is inlined, each function of
the chain is a frame of its own on the stack (no arguments: there was no
call), at the line inside it, and the real call is at the line where the
chain starts. The stack chart can show them as calls too (the timeline's
"inlined" toggle), and the line a record is at names the function its code
is inlined from.

## Syscalls

A syscall is a call of `wasm_syscall` (its arguments: the number and the
syscall's own) up to its return (the result), on one task.

## Memory as it was

Every store has the old value. `mem::History` indexes them by address.
Memory as it was after record `i` is memory at the end of the recording
with **every later store undone**, byte by byte: the oldest later store
that covers a byte has its value (`PastMemory`). The page freezes a copy of
kernel memory when recording stops (the kernel goes on, unrecorded), so
"the end" stays put.

The same index answers "who last wrote this?": the newest store to a range
at or before the cursor, and the line it was on (where `lines` says the
task was then).

What it cannot show: values only in wasm locals (registers, in effect),
and old contents of bulk writes beyond 256 bytes (known to be unknown).

## Records keep their names

The ring goes on while the page looks at a snapshot, so indexes shift from
one snapshot to the next. Each record's **id** counts the pairs ever
written up to it: the same record has the same id in every snapshot, so a
selection (a syscall, a program's run) is found again.
