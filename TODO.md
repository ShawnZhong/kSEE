# TODO

What is decided but not done yet. Done items leave this file (git log has
them).

## Page

- **Stack view** jumps as the pointer moves over a bar with many calls
  under it; a design that keeps it steady (fixed height and preview-only
  modes were tried and rejected).

## Build and repository

- **Kconfig / defconfig**: can `arch/wasm/configs/defconfig` be derived
  from Kconfig defaults (`def_bool y`, `select`), and what in
  `arch/wasm/Kconfig` can go.
- **Crate organization**: ksee-core holds the formats (tables, types,
  trace, arch) and the replay (recording, stack, mem, view), the line
  language (program) and the page's text helpers (highlight, text):
  group them by module (format/, replay/, ...) rather than split crates.
- **Plugin** (`plugin/ksee-trace.cpp`): look for what can be simpler.
