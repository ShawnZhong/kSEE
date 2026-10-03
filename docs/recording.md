# The recording

Most of what the kernel does is recorded by the compiler:
`plugin/ksee-trace.cpp`, an LLVM pass plugin, instruments every kernel
function the same way, with no settings and no list of functions. The runtime
adds the initial task and host-side stores to the same ring. The format is
specified at the top of the plugin and extended by the shared tags in
`ksee-core`; `trace.rs` decodes it.

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
  stores) call the `ksee.bulk(addr, len, line, from)` import, which logs
  their old contents the same way. A store that copies (a value loaded and
  stored as it is, a memcpy) says where its bytes come from.

A function that loops but writes no memory the host must undo and calls
nothing (a checksum, a search) is **sealed**: only its call and its
returns are recorded, not each block of each turn. That is enough to
rebuild everything else ([replay.md](replay.md)). There are no
tracepoints, no syscall hooks, nothing per subsystem.

## The ring

```c
struct { u32 pos; u32 pad; u32 on; u32 pad; u32 ring[RING_WORDS]; } __ksee_trace;
```

A record is a head pair of words and continuation pairs. The second word's
top nibble tags the pair, so a reader can start at any even offset (the
ring may have overwritten the start of a record) and skip to the next head:

| Pair | Record |
|---|---|
| `[fn, 1<<28 \| n]` | block n of fn; block 0 is a call, followed by its arguments, then its frame base |
| `[task, 3<<28]` | the `task_struct` address running when a requested syscall's recording begins |
| `[fn, 4<<28]` | a return from fn; the value follows (low, then high for one wider than 32 bits) |
| `[lhs, 6<<28 \| line<<4 \| p]` | a comparison deciding a branch (or a select), on its block's line `line`: lhs, then rhs follows (for 64-bit ones, then lhs's high word, rhs's); p its predicate (LLVM's icmp's, from eq) |
| `[addr, 7<<28 \| line<<4 \| k]` | a store of 1<<k bytes at addr, on its block's line `line` (its L line's items, from 0); the old value follows; k = 7: a range whose old contents are not logged, followed by its length; then, if it copies, where its bytes come from |
| `[value, 9<<28 \| line<<16 \| k]` | variable k of the function running (its V line) takes value, on its block's line `line` (12 bits); its high word follows, for a wide one |
| `[value, 8<<28]` | a continuation |

`fn` is a hash of the function's name (for one local to its file, of
"file:name"). `pos` counts words ever written and is stored with release
order after each record, so a reader sees whole records. One writer runs at a
time because the kernel is uniprocessor.

Block, return, comparison, variable, and instrumented store records come from
the LLVM pass. The runtime emits a `Task` record immediately when
`ksee.begin` starts a requested syscall recording, before enabling full
probes. It also emits store records for host writes into kernel memory, such
as a runner request or lesson variable. The initial task record identifies a
recording that may contain no preceding context switch; later task changes
are inferred from kernel call and return records.

Probes record **only while `on` is nonzero**, at one of two levels. `1`:
everything, while a runner is between `ksee.begin` and `ksee.end`
(user/prog.c: around the syscall the page asked for), so a recording is
that syscall, entry to return, and whatever the kernel does meanwhile
(other tasks it wakes, timers, the clock skipping in idle). `2`: stores
only, between calls, so memory stays exact throughout. Otherwise each
probe costs a load and a branch.

## What the kernel says about itself

The instrumented kernel describes its probes in a custom section,
`ksee.probes` (lines of text, one part per compilation unit,
concatenated by wasm-ld):

- `U`: a unit's start; `P k path`: file k; `I k …`: an inline site (the
  function, and where it is inlined into the next one out); `R words`: the
  ring's size;
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
  parameter arg throughout, with no records.

The plugin cannot say the rest itself: a local's frame offset is the
backend's (after the pass), a global's address the linker's, and a type is
one across all the units. The build's pack (crates/ksee-cli/src/pack) reads the kernel's
DWARF and adds `ksee.types`: the types (deduplicated across units), globals,
each function's parameters with their types (`P`, as C declares them) and
its locals with their types and, those in memory, their offsets from the
frame base (`Q`, the offset empty for one in a wasm local), the syscalls
this kernel builds (its `sys_call_table`'s functions: `__wasm_sys_NAME`,
NULL for none), constants from the UAPI headers (their `#define`s, and
the enums' values: for the page's calls, written as in C), and `W name
file line` lines: where a function (an inline one too, which has no probe),
a struct, union, enum or typedef, a global, or a member (as
`struct.member`) is declared (the page's Ctrl+click and F12 go there).
The page's kernel is a copy of vmlinux with that section and without
DWARF, plus the sources the probes name.
