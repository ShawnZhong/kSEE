# The recording

What the kernel does is recorded by the compiler: plugin/ksee-trace.cpp, an
LLVM pass plugin, instruments **every kernel function the same way**, with
no settings and no list of functions, and writes one ring of records. The
format is specified at the top of the plugin; ksee-core's trace.rs decodes
it.

## What is recorded

The pass runs last, after optimization (`OptimizerLast`), so the probes
describe the code that actually runs, and cannot stop the optimizer from
removing code the kernel relies on being removed. The kernel is built at
`-Os` with `-finline-hint-functions`: only what the source marks `inline`
is inlined, so the compiler's own inlining does not hide calls (with their
arguments) as inlined code. (Measured on the examples: 11,500 functions
with probes rather than 9,000, 40% of the lines recorded inlined rather
than 53%, for 14% more records; with no inlining but `__always_inline`,
8% inlined but 2.3 times the records, every small helper a call.)

Every function gets:

- at the start of each **basic block**, a block record. The entry block's
  is the **call**, followed by its **arguments**: the source's parameters,
  mapped through the debug records (so an argument the optimizer removed
  is known to be missing, not misread), a word each, two for one wider
  than 32 bits (a `loff_t`), and its **frame base** (the stack
  pointer after its prologue, `__builtin_frame_address(0)`): its locals in
  memory are at their offsets from it, which DWARF knows (`ksee pack`
  writes them, see below), so a stack address is a frame's, and a local's
  lifetime is its call's. The entry block's first line is the function's
  signature, so a call begins there, before its first statement;
- before each **return**, a return record with the value;
- before each **store** (its stack slots' too: a slot is reused by later
  calls, and a frame's locals must be undone to be seen as they were), a
  store record with the **old value** and the **line** it is on (which of
  its block's lines: so a line step lands at its first store, and a store
  is attributed to its line). Bulk writes (memcpy, memset, wider stores)
  call the `ksee.bulk(addr, len, line)` import, which logs their old
  contents the same way. A struct passed by value (`byval`) is copied by
  the pass itself, a memcpy into a slot of the caller's (recorded as
  any), passed as a plain pointer (wasm's calling convention for it
  anyway): the backend would otherwise make the copy after the pass, and
  its stores would go unrecorded (memory as it was wrong where it lands).

A function that loops but writes no memory and calls nothing (a checksum,
a search: `crc32c`, `_find_next_bit`, `memcmp`; about 130 of them) is
**sealed**: only its call and its returns are recorded, not each block of
each turn (the source view says so). That is enough to rebuild everything
else ([replay.md](replay.md)). There are no tracepoints, no syscall hooks,
nothing per subsystem.

## The ring

```c
struct { u32 pos; u32 pad; u32 on; u32 pad; u32 ring[RING_WORDS]; } __ksee_trace;
```

A record is a head pair of words and continuation pairs. The second word's
top nibble tags the pair, so a reader can start anywhere (the ring may have
overwritten the start of a record) and skip to the next head:

| Pair | Record |
|---|---|
| `[fn, 1<<28 \| n]` | block n of fn; block 0 is a call, followed by its arguments (a word each, two for a wide one), then its frame base |
| `[task, 3<<28]` | the runtime's: task (a task_struct) is running (where a recording begins: no switch says so) |
| `[fn, 4<<28]` | a return from fn; the value follows |
| `[dst, 5<<28]` | a copy: the store (or bulk write) after it writes dst..dst+len with what was at src (a value loaded and stored as it is, a memcpy); src and len follow |
| `[lhs, 6<<28 \| line<<4 \| p]` | a comparison that decides a branch (or a select), on its block's line `line`: lhs, then rhs follows (32 bits each); p its predicate (LLVM's icmp's, from eq) |
| `[addr, 7<<28 \| line<<4 \| k]` | a store of 1<<k bytes at addr, on its block's line `line` (its L line's items, from 0); the old value follows (k = 7: a range of unknown old contents; its length follows) |
| `[value, 8<<28 \| i]` | continuation i |

`fn` is a hash of the function's name. `pos` counts words ever written and
is stored with release order after each record, so a reader sees whole
records. One writer at a time: the kernel is uniprocessor, and the runtime
writes its own records only while the kernel runs (one thread).

Probes record **only while `on` is set**, at one of two levels. `1`:
everything, while a runner is between `ksee.begin` and `ksee.end`
(user/prog.c: around the syscall the page asked for), so a recording is
that syscall, entry to return, and whatever the kernel does meanwhile
(other tasks it wakes, timers, the clock skipping in idle). `2`: stores
only, between calls (from the first call on): what the kernel does then
(the runner's way back to its wait, idle) is not shown, but every write
is still undone when going back, so memory as it was stays exact: a
call's locals are on the runner's kernel stack, which its wait reuses.
Otherwise each probe costs a load and a branch.

## What the kernel says about itself

The instrumented kernel describes its probes in a custom section,
`ksee.probes` (lines of text, one part per compilation unit):

- `F fn name FILE:line known sealed wide`: a function, which of its
  parameters call records carry, whether it is sealed (see above), and
  which parameters take two words;
- `B fn n FILE:line:column`: where block n starts;
- `L fn n …`: the source lines block n runs, in order, in stretches between
  the calls it makes; a line of inlined code carries its inline chain:
  each function it is inlined from, innermost first, as an inline site
  (`I k function FILE:line`: the function, and the line where it is
  inlined into the next one out);
- `U`, `P k path`, `I k …`: a unit's files and inline sites; `R words`:
  the ring's size.

The plugin cannot say these itself: a local's frame offset is the
backend's (after the pass), a global's address the linker's, and a type is
one across all the units (each unit the plugin sees has its own copy).
`ksee pack` (crates/ksee-cli) reads the kernel's DWARF and adds
`ksee.types`: the types (deduplicated across units, as pahole does for
BTF), globals, each function's parameters with their types (`P fn name
param:type,…`) and its locals in memory with their types and offsets
from the frame base (`Q fn var:type:offset,…`; `fn` the function's id as
the probes have it: its name's hash, or for a function local to its file
the unit's "file:name"'s, so that the functions of one name in several
files each have their own; from the variables' DWARF locations: `DW_OP_fbreg`, or
`DW_OP_WASM_location 0 <the frame base's local>` plus a constant, in
pieces for a struct's members; the locals of calls inlined into it too;
a function inlined elsewhere has them on its out-of-line copy), the
syscalls' names, constants from the UAPI headers (`O_*`, `AT_*`,
`CLOCK_*`, `CLONE_*`, `FUTEX_*`, errno's `E*`, …: for the page's calls,
written as in C), and `W name file line` lines: where a function (an
inline one too, which has no probe), a struct, union, enum or typedef, a
global, or a member (as `struct.member`) is declared, from DWARF's
decl_file/decl_line (the page's Ctrl+click and F12 go there: `Machine.definition(name)`, `.member` a member
of any struct). The page's kernel is a copy of vmlinux with that
section and without DWARF, plus the sources the probes name.
