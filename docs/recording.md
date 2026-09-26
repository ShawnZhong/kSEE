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
  is known to be missing, not misread), and its **frame base** (the stack
  pointer after its prologue, `__builtin_frame_address(0)`): its locals in
  memory are at their offsets from it, which DWARF knows (`ksee pack`
  writes them, see below), so a stack address is a frame's, and a local's
  lifetime is its call's;
- before each **return**, a return record with the value;
- before each **store** (its stack slots' too: a slot is reused by later
  calls, and a frame's locals must be undone to be seen as they were), a
  store record with the **old value**. Bulk writes (memcpy, memset, wider
  stores) call the `ksee.bulk(addr, len)` import, which logs their old
  contents the same way.

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
| `[fn, 1<<28 \| n]` | block n of fn; block 0 is a call, followed by its arguments, then its frame base |
| `[fn, 4<<28]` | a return from fn; the value follows |
| `[addr, 7<<28 \| k]` | a store of 1<<k bytes at addr; the old value follows (k = 7: a range of unknown old contents; its length follows) |
| `[value, 8<<28 \| i]` | continuation i |

`fn` is a hash of the function's name. `pos` counts words ever written and
is stored with release order after each record, so a reader sees whole
records. One writer at a time: the kernel is uniprocessor, and the runtime
writes its own records only while the kernel runs (one thread).

Probes record **only while `on` is set**: the runtime turns it on for each
call the page asks a runner to make (Machine.call), and off when the kernel
idles for the page's input again: until the next call, memory stays as the
recording leaves it. Otherwise each probe costs a load and a branch.

## What the kernel says about itself

The instrumented kernel describes its probes in a custom section,
`ksee.probes` (lines of text, one part per compilation unit):

- `F fn name FILE:line known sealed`: a function, which of its parameters
  call records carry, and whether it is sealed (see above);
- `B fn n FILE:line:column`: where block n starts;
- `L fn n …`: the source lines block n runs, in order, in stretches between
  the calls it makes; a line of inlined code carries its inline chain:
  each function it is inlined from, innermost first, with the line where
  that function is inlined into the next one out;
- `U`, `P k path`: a unit's files; `R words`: the ring's size.

`ksee pack` (crates/ksee-cli) reads the kernel's DWARF and adds
`ksee.types`: the types (deduplicated across units, as pahole does for
BTF), globals, each function's parameters with their types and its locals
in memory with their types and offsets from the frame base (`Q name
var:type:offset,…`, from the variables' DWARF locations: `DW_OP_fbreg`, or
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
