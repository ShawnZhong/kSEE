# TODO

What is decided but not done yet. Done items leave this file (git log has
them).

## Page

- **Stack view** jumps as the pointer moves over a bar with many calls
  under it; a design that keeps it steady (fixed height and preview-only
  modes were tried and rejected).

## Trace format

- **Records read backwards, no continuation words.** Each value now takes
  two words ([v, CONT]) so a reader can resynchronize where the ring
  overwrote a record's start. Ending each record with its head
  (`values..., a, tag | n`) lets the reader walk back from `pos`, always a
  boundary: half the words for values, a simpler writer (`ksee.c`) and
  decoder (`trace.rs`); ids stay word positions.

## Build and repository

- **Kconfig / defconfig**: can `arch/wasm/configs/defconfig` be derived
  from Kconfig defaults (`def_bool y`, `select`), and what in
  `arch/wasm/Kconfig` can go.
- **Crate organization**: ksee-core holds the formats (tables, types,
  trace, arch) and the replay (recording, stack, mem, view), the line
  language (program) and the page's text helpers (highlight, text):
  group them by module (format/, replay/, ...) rather than split crates.
- **Plugin** (`plugin/ksee-trace.cpp`): look for what can be simpler.
