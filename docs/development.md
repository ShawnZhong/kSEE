# Development

This is the contributor reference for repository structure, build outputs,
commands, and tests. Read [design.md](design.md) first for the constraints that
shape the code.

## Repository map

| Path | Responsibility |
|---|---|
| `arch/wasm/` | The port: the WebAssembly architecture, copied into `linux/arch/wasm/` by the build. |
| `linux.patch` | The port's few edits to Linux's own files, against stable `v7.2.7`. |
| `linux/` | Fetched working kernel tree with the patch applied; created on the first build. |
| `plugin/` | LLVM 22 pass that instruments kernel functions. |
| `crates/ksee-core/` | `no_std` trace decoding, packed tables, reconstruction, views, program parsing/evaluation, text parsing, and source highlighting. |
| `crates/ksee-host/` | Browser host for the kernel: shared memory, JSPI task scheduling, programs, snapshots, and the `Machine` API. |
| `crates/ksee-page/` | Dioxus UI built into the same WebAssembly module as the runtime. |
| `crates/ksee-cli/` | Build, packing, test, browser driver, server, deployment, and patch commands. |
| `user/` | Freestanding `/init`, syscall runner, headers, and lesson JSON. |
| `web/` | Static frame, styles, and JavaScript loader. |
| `target/site/` | Generated deployable site. |

The crate dependencies are intentional:

```text
ksee-core <- ksee-host <- ksee-page
    ^                        |
    +------------------------+    (the page also uses core's views directly)

ksee-cli                          (no ksee crate: it writes the formats)
```

`ksee-page` depends on both `ksee-host` and `ksee-core`; the host depends on
core. The CLI shares no code with them: it writes the formats core reads
(`ksee.types`, see `types.rs`; the probes' function ids, FNV-1a of the name,
see `tables.rs` and the plugin), so the two meet only in those texts, which the
kernel suite checks end to end.

## Build pipeline

`./ksee.sh` first builds `ksee-cli` in release mode with Cargo's locked
dependency set, then runs it. `ksee build` performs these stages:

1. `make` (the Makefile: the kernel's half, C and LLVM): require LLVM 22; fetch
   Linux stable `v7.2.7` into `linux/` if it is absent, verify the pinned
   commit, apply `linux.patch` and copy `arch/wasm/` in; build `plugin/ksee-trace.so`; run kbuild
   for the WebAssembly kernel and its UAPI headers (a hash of the plugin's
   source is part of the kernel flags, so changing the pass rebuilds the
   kernel); compile freestanding `user/init.c` and `user/prog.c`, make the ext4
   image of `user/disk/`, and archive them with `user/note.txt` as
   `user/initramfs.list` says (the kernel's `gen_init_cpio`).
2. Create `target/site/` and link the current static files from `web/`.
3. Pack `linux/vmlinux`: combine probe descriptions with DWARF-derived types,
   symbols, syscalls, constants, and declaration locations; copy referenced
   kernel sources; strip DWARF; and write `target/site/vmlinux.gz`, then
   `target/site/initramfs.cpio.gz`.
4. Combine `user/examples/*.json` in course order into
   `target/site/examples.json`.
5. Build `ksee-page` for `wasm32-unknown-unknown` with `--locked --release`.
   Cargo links `ksee-host` and `ksee-core` into the same outer module, then
   wasm-bindgen writes `target/site/pkg/`.

Most stages ask their owning build system for dependency tracking. Smaller
packing steps compare output and input modification times.

### LLVM selection

The build requires LLVM major version 22. The Makefile puts
`/usr/lib/llvm-22/bin` ahead of `PATH`; without that installation, the LLVM on
`PATH` is used. It checks `llvm-config --version` before building the plugin. It
does not track the selected LLVM installation as an incremental-build input,
so use one LLVM 22 toolchain for a checkout after its first build. If you
switch toolchains, rebuild the native plugin and kernel outputs rather than
reusing artifacts produced by the previous installation.

Cargo build and test commands use `--locked`, so `Cargo.lock` is the selected
dependency set.

## Commands

`ksee.sh` is the front door: it builds the release CLI with locked dependencies
and passes all arguments through.

| Command | Behavior |
|---|---|
| `./ksee.sh` | Build changed outputs, then serve `target/site/` on port 8000. |
| `./ksee.sh build` | Build the plugin, kernel, browser module, programs, metadata, lessons, and site. |
| `./ksee.sh serve [--port=N]` | Serve the existing `target/site/` without rebuilding it. |
| `./ksee.sh test [--ui \| NAME ...]` | Run native tests, build, then run the selected browser suites in headless Chrome; the default is `kernel`. |
| `./ksee.sh deploy` | Build, then publish the generated site to the remote `gh-pages` branch. |
| `./ksee.sh patch` | Write `linux.patch` from Linux's own files' difference between `linux/` and `v7.2.7` (`make patch`). |

`build`, `test`, and the default command can fetch the kernel on their first
run. `serve` expects an existing site.

## Generated outputs

Do not edit generated output as source:

| Output | Contents |
|---|---|
| `linux/vmlinux` | Raw instrumented Kbuild kernel with DWARF; retained with the Linux build cache in CI. |
| `target/site/` | Normal site served locally and deployed. |
| `target/site/pkg/` | wasm-bindgen output for the combined page/runtime module. |
| `target/site/vmlinux.gz` | Instrumented, packed kernel without DWARF. |
| `target/site/src/` | Kernel source files referenced by probe metadata. |
| `target/site/initramfs.cpio.gz` | `/init`, `/bin/prog`, root fixture, and ext4 image. |
| `target/site/examples.json` | Built lesson index and content. |
| `target/vmlinux` | Uncompressed packed kernel, with metadata injected and DWARF removed, used to produce the site copy. |
| `target/initramfs/` | Intermediate programs, ext4 image, fixture tree, and archive. |
| `target/chrome/` | The tests' browser, Chrome's headless shell (`make chrome`). |

## Page modules and state

The page is Rust/Dioxus, not a JavaScript layout shell. It is linked into the
runtime's WebAssembly module and calls `Machine` directly.

| Module | Responsibility |
|---|---|
| `lib.rs` | Mounting, boot handoff, the booted `Machine` and the lessons (thread-locals), context providers, URL synchronization, the single active lesson run, and top-level layout. |
| `lesson.rs` | Lesson schema, navigation, execution algorithm, outcomes, links, tries, and rendering. |
| `editor.rs` | In-place program editing, completions, signatures, and validation. |
| `chart.rs` | Whole-recording overview, stack chart, task strip, transport, zoom/pan input, and function search. |
| `source.rs` | Virtualized kernel source, executed lines, values, stores, run-to-line, and definition navigation. |
| `stack.rs` | Line/frame stepping, keyboard controls, stack frames, values, and origins. |
| `panels.rs` | Stack, Tasks, Objects, and Console tabs. |
| `layout.rs` | Resizable panes and persisted dimensions. |
| `state.rs` | Semantic state transitions, the snapshot identity, navigation, lesson program, and derived views. |
| `test/lessons.rs` | Every lesson and try on a fresh kernel: its calls finished, its links found. |
| `test/ui.rs` | Browser-level page interaction suites. |
| `test/dom.rs` | DOM queries, synthetic input, waiting, and assertions shared by UI suites. |
| `test/mod.rs` | URL dispatch and result reporting to the CLI's browser driver. |

The kernel's own suite is `ksee-host`'s `test/` (harness, trace, program,
metadata, lifecycle, timings): it checks the Machine, with no page. The tests
are always in the shipped module (about 80 KB gzipped); a URL naming one,
`#test=NAME`, runs it.

The central state is the selected syscall, reconstructed line cursor, selected
stack frame, and lesson program. A chart hover supplies a temporary preview. Dioxus memos derive the syscall, line sequence,
chart, stack, and frame from those signals and the published snapshot identity.
The root cancels the previous lesson run as it starts another: a cancelled
task's future is dropped, so a superseded run never publishes.

The runtime exposes a closure-based boundary: `Machine::view` lends a
`ksee_core::view::View` over the last snapshot (its views of a moment, the
Stack's, take the record and rebuild memory as it was), while
`Machine::at_moment` also lends that memory, for reading a value. The page does
not own a second replay model.

Chart window calculations are split at the ownership boundary. Dioxus event
handling and rendering remain in `ksee-page`, while `wheel_window`,
`clamped_pan` and their native unit tests live in `ksee-core`. The page and
the host build only for `wasm32`: the browser suites test them, and the
workspace's default members (a bare `cargo test`) are the native crates.

## Tests

`./ksee.sh test` runs layers in this order:

1. `cargo test --locked -p ksee-core -p ksee-cli` runs native
   reconstruction, view, program-language, source/text, chart-window, packing,
   and CLI tests.
2. The site is built, as `ksee build` builds it: the tests are in its module.
3. Headless Chrome runs the kernel suite (`ksee-host`): it boots Linux,
   exercises recorded syscalls, and checks stacks, lines, memory history,
   origins, tasks, and timings; then the lessons suite, every lesson and try.
4. With `--ui`, it also runs the read, changed-program/history, pipe, and
   phone-layout suites through the DOM. Test names can also be given directly
   for focused runs, such as `./ksee.sh test read`.

CI separates launcher compilation, the kernel/site build, and tests. Its test
step uses `ksee test --built`, which runs native tests and the selected browser
suites on the site already built.

The browser is Chrome's headless shell at the Makefile's `CHROME_VERSION`,
fetched into `target/chrome/` by `make chrome`, which `ksee test` runs.

The CLI serves the test site on a temporary local port. Each browser suite
posts `ok` or `fail` and its log back to that server, within five minutes.

For a focused native run:

```sh
cargo test --locked -p ksee-core
cargo test --locked -p ksee-cli
```

## Linux patch workflow

The port is `arch/wasm/` (the architecture: its own files) and `linux.patch`
(its few edits to Linux's own files). The first build creates `linux/` from
the pinned stable tag and applies the patch once; every build copies
`arch/wasm/`'s changed files into `linux/arch/wasm/`. To change the port:

1. Edit `arch/wasm/` in the repository (not its copy in `linux/`); change
   Linux's own files, in `linux/`, only where the port requires it.
2. Build and test the change.
3. If Linux's own files changed, run `./ksee.sh patch` to regenerate
   `linux.patch` from their difference against `v7.2.7` (files the port adds
   are `arch/wasm/`'s, untracked in `linux/`'s git).
4. Review and commit `arch/wasm/` and `linux.patch` with the related source
   and documentation.

The normal build never rewrites the patch. If you need to apply a revised patch
to a clean kernel tree, remove or relocate the generated `linux/` tree first;
the next build fetches and applies it again.
