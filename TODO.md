# TODO

What is decided but not done yet. Done items leave this file (git log has
them).

## Page

- **Header**: kSEE centered; the lesson buttons at the upper left named
  (not bare ◀ ▶); at the upper right, the build id, linking to its source
  (the commit on GitHub).
- **No ad-hoc labels**: a syscall entry's bar says `wasm_syscall · write`
  (chart.rs, `Bar.label`): one function's special case; the bar should say
  its function as every bar does.
- **Stack view** jumps as the pointer moves over a bar with many calls
  under it; a design that keeps it steady (fixed height and preview-only
  modes were tried and rejected).

## Tests

- **The kernel suite belongs to ksee-host** (it tests the Machine, not the
  page): move `crates/ksee-page/src/test/kernel/` there behind a feature;
  the page keeps its DOM suites; one runner for both.

## Build and repository

- **Kconfig / defconfig**: can `arch/wasm/configs/defconfig` be derived
  from Kconfig defaults (`def_bool y`, `select`), and what in
  `arch/wasm/Kconfig` can go.
- **Crate organization**: ksee-core holds the formats (tables, types,
  trace, arch) and the replay (recording, stack, mem, view), the line
  language (program) and the page's text helpers (highlight, text):
  group them by module (format/, replay/, ...) rather than split crates.
- **Plugin** (`plugin/ksee-trace.cpp`): look for what can be simpler.
