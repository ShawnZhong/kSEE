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

A frame's arguments and its frame base are the words after its block 0
(`trace::Args`, by the call's record; `Tables::values` joins a wide
argument's two); its locals in memory are at their offsets from the base
(`Types::locals`), and those it keeps in wasm locals, and a parameter it
changed, are as its last var record in the call says (`Calls::vars_of`:
each call's, indexed in the same pass), and so are those of the code
inlined into it (by the inlined function's name: of two inlined copies of
one function in a call, the later's values).
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
`at` (the record just before it runs, as a debugger stops before a line:
before the first record on it or a later line of its stretch, a store, a
comparison or a variable set, each saying its line; with none, after the
stretch's last; a stretch's first line at the block's record: the cursor
goes there, and memory and the variables are as they were) and how deep it runs (calls deep, and
inlined deep): the level stepping over or out of a call compares. A
call's last line before it returns says what it returned (`ret`): the
optimizer leaves `return x;` no code of its own (its value joins the
function's one exit, whose line is the closing brace), so that is where
the page shows it. A
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
task was then); and the stores of a span of records (`stores`), in
order, each by the region it hit, named as the code names it (a call's pointer
argument to a struct, a local and what a pointer local points to as the
call ends, the task running and what its pointer members point to, a
global: "file->f_pos in vfs_read", "prog(19)->files->next_fd" (the task running as the span begins, by its name); what the
code did not name, by its address). A call's local, its frame, and an
object a pointer reaches in a frame on the stack, are theirs only while
that call runs (the slot is another's before and after). Named once
per span, a store is printed only where it is shown: as the member it
wrote (the one its first byte is in: `Types::leaf`), as its type before
and after (memory as it was). What a call did
(`objects`) is its stores by object, each to the outermost object holding
it (a page's refcount is the page's `_refcount.counter`), the kernel's
data, not a stack's (a call's locals, and what a pointer reaches on a
stack, are the Stack's): each member (a store copying several, each of
them: `Types::leaves`) from what it was as the span began to what it is as it ends,
and how: a run a function writing it in turn, from what to what (one
that left it as it was, not; a member no run changed, not); a pointer, the
object it points to as it ends; and each object under the first that
points to it then (the members of pointer type, `Types::pointers`, whose
value is inside another, of its group: those left changed, and those left
as they were, every change undone).

A store with no line of its own (the compiler gave it none, or one past
its stretch) is on the line its stretch ends on.

## Where a value came from, and why a branch went

A store that copies (a value loaded and stored as it is, a memcpy) says
where its bytes come from (each store of a memcpy its own). So a value's way through memory can be
followed back (`origin`): its last write, and while that was a copy, the
last write of what it copied, and so on: the bytes a read returns, from
the page cache, which a write filled from its caller's buffer. It ends
where a value was computed (in wasm locals), or at memory written before
the recording.

A comparison that decides a branch has a Compare record: its operands and
its predicate. A line step says the comparisons it made (`tests`: "18 >=
0 ✓"): the values the branch went by, which live only in wasm locals
otherwise. (The comparison as compiled: the optimizer may have turned
the source's around, so it says what was compared, not which way the
source's `if` went.)

What it cannot show: values only in wasm locals (registers, in effect)
that are no named variable's (an intermediate, `a + b` before it is stored),
and old contents of bulk writes beyond 4096 bytes (their record says
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
