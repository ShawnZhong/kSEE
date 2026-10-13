# The recording

Most of what the kernel does is recorded by the compiler:
`plugin/ksee-trace.cpp`, an LLVM pass plugin, instruments every kernel
function the same way, with no settings and no list of functions; its probes
call the kernel's writers, `arch/wasm/kernel/ksee.c`, which holds the ring
(the host has it write the initial task and its own stores too). The format
is specified at the top of the plugin and in `ksee.c`; `trace.rs` decodes it.

## What is recorded

The pass runs last, after optimization, so the probes describe the code
that actually runs, and cannot keep the optimizer from removing code the
kernel relies on being removed. The kernel is built with
`-finline-hint-functions`: only what the source marks `inline` is inlined,
so the compiler's own inlining does not hide calls (with their arguments)
as inlined code.

Every function gets:

- at the start of each **basic block**, a block record. The entry block's
  is the **call**, followed by its **arguments** (up to seven: the source's
  parameters, mapped through the debug records, so an argument the
  optimizer removed is known to be missing, not misread, a word each, two
  for one wider than 32 bits) and its **frame base** (the stack pointer
  after its prologue: its locals in memory are at their DWARF offsets from
  it, which the build's pack writes, see below). The entry block's first line is the function's signature, so a call
  begins there, before its first statement;
- before each **return**, a return record with the value;
- where the code sets a **named variable it keeps in a wasm local**, not
  memory (a `#dbg_value`: most of a function's scalars, fewer than one
  local in twenty is in memory), a var record with the value: so every
  local, and a parameter the function changes, has its value at any moment,
  and so do the parameters and locals of code inlined into it. Not a set
  that says nothing new: one the debug info computes, or of a variable
  that only ever takes one value after another set of it, or of an inlined
  call's parameter passed one of the call's own arguments (the call record
  has it);
- before each **store** (its stack slots' too: a slot is reused by later
  calls, and a frame's locals must be undone to be seen as they were), a
  store record with the **old value** and the **line** it is on (which of
  its block's lines: so a line step lands where its first store is, and a
  store is attributed to its line). Bulk writes (memcpy, memset, wider
  stores) call the kernel's `__ksee_bulk(addr, len, line, from)`
  (arch/wasm/kernel/ksee.c), which logs all their old contents the same way,
  in pieces of up to 8 bytes. A store that copies (a value loaded and
  stored as it is, a memcpy) says where its bytes come from.

A function that loops but writes no memory the host must undo and calls
nothing (a checksum, a search) is **sealed**: only its call and its
returns are recorded, not each block of each turn. That is enough to
rebuild everything else ([replay.md](replay.md)). There are no
tracepoints, no syscall hooks, nothing per subsystem.

## The ring

```c
struct { u32 pos; u32 words; u32 on; u32 ring[RING_WORDS]; } trace;  /* ksee.c; words: RING_WORDS */
```

A record is its values (32-bit words), then its head, two words `[a, tag]`.
The tag's top nibble says what the record is, the next how many values come
before the head (ksee.c adds it), the other 24 bits its fields. So a reader
walks back from `pos`, always a record's end, record by record, and stops at
one the ring has partly overwritten:

| Head | Record |
|---|---|
| `[fn, 1<<28 \| n]` | block n of fn; block 0 is a call: its values its arguments, then its frame base |
| `[task, 3<<28]` | the `task_struct` address running as a call's recording begins |
| `[fn, 4<<28]` | a return from fn; its values the value (low, then high for one wider than 32 bits; none for a void function) |
| `[lhs, 6<<28 \| line<<4 \| p]` | a comparison deciding a branch (or a select), on its block's line `line`: lhs, its values rhs (for 64-bit ones, then lhs's high word, rhs's); p its predicate (LLVM's icmp's, from eq) |
| `[addr, 7<<28 \| line<<4 \| k]` | a store of 1<<k bytes at addr (k < 4), on its block's line `line` (its L line's items, `\|` apart, from 0; 20 bits); its values the old value (low, then high for 8 bytes), then, if it copies (a value loaded and stored as it is, a memcpy), where its bytes come from |
| `[value, 9<<28 \| line<<12 \| k]` | variable k (12 bits) of the function running (its V line) takes value, on its block's line `line` (12 bits); its high word its value, for a wide one |

`fn` is a hash of the function's name (for one local to its file, of
"file:name"). `pos` counts words ever written and is stored with release
order after each record, so a reader sees whole records. One writer runs at a
time because the kernel is uniprocessor.

Block, return, comparison and variable records come from the LLVM
pass's probes, store records from the writes they see, all written by
`ksee.c`. Only the kernel writes its ring: the runtime calls the kernel's
`ksee_record` export (see below), which writes a `Task` record each time a
call's recording begins; and the host's own writes into kernel memory (a
runner request, a lesson variable) go through its `ksee_bulk` export first. The initial task record identifies a
recording that may contain no preceding context switch; later task changes
are inferred from kernel call and return records.

Probes record from the program's first line (a declaration's write, or a
call's entry), at one of two levels, as the host says
(`ksee_record`). `1`: everything, while a call the page asked for is under
way (from the kernel's `syscall_enter` to its `syscall_exit`, around
`wasm_syscall`, arch/wasm/kernel/syscall.c): the syscall, entry to
return, and whatever the kernel does meanwhile (other tasks it wakes or
waits for, the journal, timers, the clock skipping in idle), from a `Task`
record. `2`: stores only, between calls, so memory stays exact: the
runners' waits for their next calls and the kernel's own work then are not
shown. Before, the boot, nothing; each probe costs a load and a branch.

## What the kernel says about itself

The instrumented kernel describes its code in a custom section,
`ksee.code` (lines of text, one part per compilation unit,
concatenated by wasm-ld):

- `U`: a unit's start; `P k path`: file k; `I k …`: an inline site (the
  function, and where it is inlined into the next one out);
- `F fn name FILE:line known sealed wide`: a function, which of its
  parameters call records carry, whether it is sealed (see above), and
  which parameters take two words;
- `L fn n …`: the source lines block n runs, in order, in stretches
  between the calls it makes (`|` where it calls: the callee runs in
  between; `@k` where the line is inlined from site k, innermost first);
  block 0's first: the function's signature;
- `V fn k of function name param arg`: variable k of fn, as its var
  records have it: `name` of `function` (fn, or one inlined into it; `of`
  its id), its parameter number (0: a local); `arg`, if nonzero: it is fn's
  parameter arg throughout, with no records;
- `S nr fn`: syscall nr calls function fn (`sys_call_table`'s entry, in
  syscall.c's unit; one this kernel does not build has no `F` line).

The plugin cannot say the rest itself: a local's frame offset is the
backend's (after the pass), a global's address the linker's, and a type is
one across all the units. The build's pack (crates/ksee-cli/src/pack) reads the kernel's
DWARF and adds the types' text (see `types.rs`): the types (deduplicated across units), globals,
each function's parameters with their types (`P`, as C declares them) and
its locals with their types and, those in memory, their offsets from the
frame base (`Q`, the offset empty for one in a wasm local), the syscalls
this kernel builds (`S nr NAME`: the plugin's `S` lines whose function,
`__wasm_sys_NAME`, has an `F` line), constants from the UAPI headers (their `#define`s, and
the enums' values: for the page's calls, written as in C), and `W name
file line` lines: where a function (an inline one too, which has no probe),
a struct, union, enum or typedef, a global, or a member (as
`struct.member`) is declared (the page's Ctrl+click and F12 go there).
The pack parses both texts (core's parser) and writes the result as one
binary file, `tables` (`flat.rs`): sorted arrays of fixed-size records, and
one text their names are offsets into, which the page reads where they are
(nothing to parse or decode but the few thousand types).
The page's kernel is a copy of vmlinux without DWARF and `ksee.code`, plus
the sources the probes name.
