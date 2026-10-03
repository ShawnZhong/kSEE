# Development

This is the contributor reference for repository structure, build outputs,
commands, and tests. Read [design.md](design.md) first for the constraints that
shape the code.

## Repository map

| Path | Responsibility |
|---|---|
| `linux.patch` | Complete `arch/wasm` port and required generic-kernel changes against stable `v7.2.7`. |
| `linux/` | Fetched working kernel tree with the patch applied; created on the first build. |
| `plugin/` | LLVM 23 pass that instruments kernel functions. |
| `crates/ksee-core/` | `no_std` trace decoding, packed tables, reconstruction, views, program parsing/evaluation, text parsing, and source highlighting. |
| `crates/ksee-runtime/` | Browser host for the kernel: shared memory, JSPI task scheduling, programs, snapshots, and the `Machine` API. |
| `crates/ksee-page/` | Dioxus UI built into the same WebAssembly module as the runtime. |
| `crates/ksee-cli/` | Build, packing, test, browser driver, server, deployment, and patch commands. |
| `user/` | Freestanding `/init`, syscall runner, headers, and lesson JSON. |
| `web/` | Static frame, styles, and JavaScript loader. |
| `target/site/` | Generated deployable site. |

The crate dependencies are intentional:

```text
ksee-core
  ^        ^
  |        |
ksee-runtime   ksee-cli
  ^            |
  |            +-- uses ksee-core while packing metadata
  |
ksee-page -- also uses ksee-core directly
```

`ksee-page` depends on both `ksee-runtime` and `ksee-core`. The runtime depends
on core. The CLI also depends on core for packing, but it is a native build
tool rather than part of the browser module.

## Build pipeline

`./ksee.sh` first builds `ksee-cli` in release mode with Cargo's locked
dependency set, then runs it. `ksee build` performs these stages:

1. Resolve the configured LLVM tools and require LLVM major version 23.
2. Fetch Linux stable `v7.2.7` into `linux/` if it is absent, verify the pinned
   commit, and apply `linux.patch`.
3. Build `plugin/ksee-trace.so` when its source is newer than the plugin.
4. Run kbuild for the WebAssembly kernel and exported UAPI headers. A hash of
   the plugin is part of the kernel flags, so changing the pass rebuilds the
   affected kernel objects.
5. Create `target/site/` and link the current static files from `web/`.
6. Build `ksee-page` for `wasm32-unknown-unknown` with `--locked --release`.
   Cargo links `ksee-runtime` and `ksee-core` into the same outer module, then
   wasm-bindgen writes `target/site/pkg/`.
7. Pack `linux/vmlinux`: combine probe descriptions with DWARF-derived types,
   symbols, syscalls, constants, and declaration locations; copy referenced
   kernel sources; strip DWARF; and write `target/site/vmlinux.gz`.
8. Compile freestanding `user/init.c` and `user/prog.c`, create the ext4 lesson
   fixture image, assemble the initramfs, and write
   `target/site/initramfs.cpio.gz`.
9. Combine `user/examples/*.json` in course order into
   `target/site/examples.json`.

Most stages ask their owning build system for dependency tracking. Smaller
packing steps compare output and input modification times.

### LLVM selection

The build requires LLVM major version 23. Defaults are Homebrew's `llvm@23` on
macOS and `/usr/lib/llvm-23/bin` elsewhere. Override that directory with an
absolute path:

```sh
KSEE_LLVM_BIN=/absolute/path/to/llvm/bin ./ksee.sh build
```

The build checks `llvm-config --version` before doing other build work. It
does not track the selected LLVM installation as an incremental-build input,
so use one LLVM 23 toolchain for a checkout after its first build. If you
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
| `./ksee.sh test [--ui]` | Run native tests, build, then run browser suites through `$CHROME`; `--ui` adds UI tests. |
| `./ksee.sh trace LESSON` | Build, run one lesson through the browser test site, and print its reconstructed steps. |
| `./ksee.sh deploy` | Build, then publish the generated site to the remote `gh-pages` branch. |
| `./ksee.sh patch` | Write `linux.patch` from the current difference between `linux/` and `v7.2.7`. |

`build`, `test`, `trace`, and the default command can fetch the kernel on their
first run. `serve` expects an existing site.

## Generated outputs

Do not edit generated output as source:

| Output | Contents |
|---|---|
| `target/site/` | Normal site served locally and deployed. |
| `target/site/pkg/` | wasm-bindgen output for the combined page/runtime module. |
| `target/site/vmlinux.gz` | Instrumented, packed kernel without DWARF. |
| `target/site/src/` | Kernel source files referenced by probe metadata. |
| `target/site/initramfs.cpio.gz` | `/init`, `/bin/prog`, root fixture, and ext4 image. |
| `target/site/examples.json` | Built lesson index and content. |
| `target/vmlinux` | Uncompressed packed kernel used to produce the site copy. |
| `target/initramfs/` | Intermediate programs, ext4 image, fixture tree, and archive. |
| `target/test/site/` | Test site whose package points at a self-test-enabled page/runtime build. |
| `target/test/build/` | Separate Cargo target directory for that self-test module. |

## Page modules and state

The page is Rust/Dioxus, not a JavaScript layout shell. It is linked into the
runtime's WebAssembly module and calls `Machine` directly.

| Module | Responsibility |
|---|---|
| `lib.rs` | Mounting, boot handoff, top-level layout, and shared `Machine`. |
| `lesson.rs` | Lesson navigation, program execution, structured task steps, outcomes, links, tries, and URL state. |
| `editor.rs` | In-place program editing, completions, signatures, and validation. |
| `chart.rs` | Whole-recording overview, stack chart, task strip, transport, zoom/pan input, and function search. |
| `source.rs` | Virtualized kernel source, executed lines, values, stores, run-to-line, and definition navigation. |
| `stack.rs` | Line/frame stepping, keyboard controls, stack frames, values, and origins. |
| `panels.rs` | Stack, Tasks, Objects, and Console tabs. |
| `layout.rs` | Resizable panes and persisted dimensions. |
| `state.rs` | Cursor, preview, selected frame, chart window, tabs, lesson program, and derived views. |
| `selftest.rs` | Kernel and reconstruction checks compiled into the test module. |
| `uitest.rs` | Browser-level page interaction suites. |

The central state is the selected syscall, the reconstructed line cursor, and
the selected stack frame. A chart hover supplies a temporary preview. Dioxus
memos derive the syscall, line sequence, chart, stack, and frame from those
signals and the current snapshot.

The runtime exposes a closure-based boundary: `Machine::view` lends a
`ksee_core::view::View` over the last snapshot, while `Machine::at_moment`
also lends reconstructed memory after a chosen record. The page does not own a
second replay model.

Chart window calculations are split at the ownership boundary. Dioxus event
handling and rendering remain in `ksee-page`, while `wheel_window` and its
native unit tests live in `ksee-core`.

## Tests

`./ksee.sh test` runs layers in this order:

1. `cargo test --locked -p ksee-core -p ksee-cli` runs native reconstruction,
   view, program-language, source/text, chart-window, packing, and CLI tests.
2. A normal full build produces the current site.
3. A self-test-enabled `ksee-page` module is built in `target/test/build/` and
   assembled into `target/test/site/`.
4. Headless Chromium runs the kernel suite. It boots Linux, exercises recorded
   syscalls and every lesson, and checks stacks, lines, memory history,
   origins, tasks, and lesson links.
5. With `--ui`, Chromium also runs the read, changed-program/history, pipe, and
   phone-layout suites through the DOM.

Set `CHROME` to the browser executable:

```sh
CHROME=/absolute/path/to/chromium ./ksee.sh test --ui
```

The CLI serves the test site on a temporary local port. Each browser suite
posts `ok` or `fail` and its log back to that server. Kernel tests have a longer
timeout than UI suites.

For a focused native run:

```sh
cargo test --locked -p ksee-core
cargo test --locked -p ksee-cli
```

## Linux patch workflow

`linux.patch` is the port. The first build creates `linux/` from the pinned
stable tag and applies the patch once. To change the port:

1. Edit the working tree under `linux/`, preferring `arch/wasm` and changing
   generic kernel code only where the port requires it.
2. Build and test the change.
3. Run `./ksee.sh patch` to regenerate `linux.patch` from the difference
   against `v7.2.7`.
4. Review and commit `linux.patch` with the related source and documentation.

The normal build never rewrites the patch. If you need to apply a revised patch
to a clean kernel tree, remove or relocate the generated `linux/` tree first;
the next build fetches and applies it again.
