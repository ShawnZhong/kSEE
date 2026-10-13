# Development

This is the contributor reference for repository structure, build outputs,
commands, and tests. Read [design.md](design.md) first for the constraints that
shape the code.

## Repository map

| Path | Responsibility |
|---|---|
| `arch/wasm/` | The port: the WebAssembly architecture, linked in as `linux/arch/wasm/` by the build. |
| `linux.patch` | The port's few edits to Linux's own files, against stable `v7.2.7`. |
| `linux/` | Fetched working kernel tree with the patch applied; created on the first build. |
| `plugin/` | LLVM 22 pass that instruments kernel functions. |
| `crates/ksee-core/` | `no_std` trace decoding, packed tables, reconstruction, views, program parsing/evaluation, text parsing, source highlighting, a program's dylink.0, and the kernel's timers. |
| `crates/ksee-host/` | Browser host for the kernel: shared memory, JSPI task scheduling, programs, snapshots, and the `Machine` API. |
| `crates/ksee-page/` | Dioxus UI built into the same WebAssembly module as the runtime. |
| `crates/ksee-cli/` | Build, packing, test, browser driver, server, deployment, and patch commands. |
| `user/` | What the kernel runs: freestanding `/init`, the syscall runner, their headers, the initramfs list, and the disk's files. |
| `web/` | The page: its static frame, styles, and JavaScript loader (`index.html`), and the lessons (`lessons/*.json`). |
| `target/site/` | Generated deployable site. |

The crate dependencies are intentional:

```text
ksee-core <- ksee-host <- ksee-page
    ^                        |
    +------------------------+    (the page also uses core's views directly)

ksee-core <- ksee-cli              (only its lesson text, for the lessons' pages)
```

`ksee-page` depends on both `ksee-host` and `ksee-core`; the host depends on
core. The CLI uses core only for a lesson's text (text.rs's pieces, the
lessons' pages); else it writes the formats core reads
(the types' text, see `types.rs`, which it parses into the page's
`tables` with core's own parser; the probes' function ids, FNV-1a of the name,
see `tables.rs` and the plugin), so the two meet only in those texts, which the
kernel suite checks end to end.

## Build pipeline

`./ksee.sh` first builds `ksee-cli` in release mode with Cargo's locked
dependency set, then runs it. `ksee build` is `make site` (in parallel): the
Makefile is the build, file to file, and its steps that are code are the
ksee command's (`ksee tables`, `ksee sources`, `ksee lessons`, `ksee bindgen`). Its stages:

1. The kernel's half, C and LLVM: require LLVM 22; fetch
   Linux stable `v7.2.7` into `linux/` if it is absent, verify the pinned
   commit, apply `linux.patch` and link `arch/wasm/` in; build the pass, `plugin/ksee-trace.cpp`, into `target/ksee-trace.so`; run kbuild
   for the WebAssembly kernel and its UAPI headers (a hash of the plugin's
   source is part of the kernel flags, so changing the pass rebuilds the
   kernel); compile freestanding `user/init.c` and `user/prog.c` and archive
   them with `user/note.txt` as `user/initramfs.list` says (the kernel's
   `gen_init_cpio`); make the disk, an ext4 image of `user/disk/`; split
   the kernel (`llvm-objcopy`): `target/vmlinux`, without DWARF and
   the `ksee` section, and `target/vmlinux.debug`, only those.
2. Pack `target/vmlinux.debug` (`ksee tables`): combine probe descriptions with
   DWARF-derived types, symbols, syscalls, constants, and declaration
   locations into the tables (`Tables::to_bytes`, `target/tables`); copy
   the kernel sources it names to `target/site/src/` (`ksee sources`); gzip `vmlinux`,
   `tables`, `initramfs.cpio` and `disk.img` into `target/site/`.
3. Write the lessons (`ksee lessons`, `lessons.rs`): `web/lessons/*.json` in course order
   into `lessons.json`, a page per lesson from `web/index.html`, and
   `sitemap.xml`.
4. Build `ksee-page` for `wasm32-unknown-unknown` with `--locked --release`.
   Cargo links `ksee-host` and `ksee-core` into the same outer module, then
   wasm-bindgen writes `target/site/pkg/` (`ksee bindgen`).

Make tracks the files; kbuild and cargo, asked each time, track their own.
A new ksee command packs the kernel again.

The kernel and the images come out the same each time. The kernel says
`7.2.7-ksee (ksee@ksee) #1` and the release's date (its tag's commit), not the
tree's state (`-dirty`: the patch applied), the machine's user and host, or
the clock. The initramfs's times are 0, and the disk's UUID and hash seed are
fixed, its times the release's (`SOURCE_DATE_EPOCH`).

### LLVM selection

LLVM 22, as [getting-started.md](getting-started.md#use-a-custom-llvm-22-build)
says. The build does not track which LLVM it used: after switching, rebuild the
plugin and the kernel (remove `target/ksee-trace.so` and `linux/`).

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
| `./ksee.sh test [--built] [--ui \| NAME ...]` | Run native tests, build (`--built`: not, the site is), then the selected browser suites in headless Chrome; the default is `kernel` and `lessons`. |
| `./ksee.sh deploy` | Build, then publish the generated site to the remote `gh-pages` branch. |
| `./ksee.sh patch` | Write `linux.patch` from Linux's own files' difference between `linux/` and `v7.2.7` (`make patch`). |

`build`, `test`, and the default command can fetch the kernel on their first
run. `serve` expects an existing site.

## Generated outputs

Do not edit generated output as source:

| Output | Contents |
|---|---|
| `target/linux/` | Kbuild's output, out of the tree (`linux/` and `arch/wasm/` stay sources): `vmlinux`, the raw instrumented kernel with DWARF, and the UAPI headers (with `gen_init_cpio`, CI's cache: see below). |
| `target/site/` | Normal site served locally and deployed. |
| `target/site/pkg/` | wasm-bindgen output for the combined page/runtime module. |
| `target/site/vmlinux.gz` | Instrumented, packed kernel without DWARF and the `ksee` section. |
| `target/site/tables.gz` | The kernel's code and type tables, parsed: flat records the page reads in place (`Tables::to_bytes`, `flat.rs`). |
| `target/site/src/` | Kernel source files referenced by probe metadata. |
| `target/site/initramfs.cpio.gz` | `/init`, `/bin/prog`, root fixture, and ext4 image. |
| `target/site/lessons.json` | Built lesson index and content. |
| `target/site/NAME.html`, `index.html` | The page, once per lesson (and the first's, `index.html`), from `web/index.html`: the lesson's title and description (its first paragraph), and its text as plain HTML for search engines and link previews (`index.html`: every lesson's title), which the page replaces once it runs. |
| `target/site/sitemap.xml` | Every page, at the address `ksee deploy` publishes the site at (the origin's GitHub Pages), for search engines (submit it in Google's Search Console). |
| `target/vmlinux`, `target/vmlinux.debug`, `target/tables` | The kernel split (`llvm-objcopy`'s): the page's, without DWARF and the `ksee` section, and those, the pack's; and its tables (the pack's), uncompressed: the site's copies' sources. |
| `target/user/` | The programs, the initramfs, and the disk's ext4 image. |
| `target/chrome/` | The tests' browser, Chrome's headless shell (`make chrome`). |

## Page modules and state

The page is Rust/Dioxus, not a JavaScript layout shell. It is linked into the
runtime's WebAssembly module and calls `Machine` directly.

| Module | Responsibility |
|---|---|
| `lib.rs` | Mounting, boot handoff, the booted `Machine` and the lessons (thread-locals), context providers, URL synchronization, the single active lesson run, and top-level layout. |
| `lesson.rs` | Lesson schema, running a program (the Machine keeps each line's outcome), links, tries, and rendering. |
| `nav.rs` | The header (lessons menu, chapter, source link, theme), the lessons before and after, and the URL (`NAME.html#p=…`). |
| `editor.rs` | In-place program editing, completions, signatures, and validation. |
| `chart.rs` | Whole-recording overview, stack chart, task strip, zoom/pan input, and function search. |
| `source.rs` | Virtualized kernel source, executed lines, values, stores, and definition navigation. |
| `stack.rs` | The transport (line and frame stepping, its buttons and keys), stack frames, values, and origins. |
| `panels.rs` | Stack, Tasks, Memory, Disk, and Console tabs. |
| `layout.rs` | Resizable panes and persisted dimensions. |
| `state.rs` | Semantic state transitions, the snapshot identity, navigation, lesson program, and derived views. |
| `test/lessons.rs` | Every lesson and try on a fresh kernel: its calls finished, its links found. |
| `test/ui.rs` | Browser-level page interaction suites. |
| `test/dom.rs` | DOM queries, synthetic input, waiting, and assertions shared by UI suites. |
| `test/mod.rs` | URL dispatch, and the result on the console for the CLI's browser driver. |

The kernel's own suite is `ksee-host`'s `test/` (harness, trace, program,
metadata, lifecycle, timings): it checks the Machine, with no page. The tests
are always in the shipped module; a URL naming one,
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
handling and rendering remain in `ksee-page`, while `wheel_window` and its
native unit tests live in `ksee-core`. The page and
the host build only for `wasm32`: the browser suites test them, and the
workspace's default members (a bare `cargo test`) are the native crates.

## Tests

`./ksee.sh test` runs layers in this order:

1. `cargo test --locked --release -p ksee-core -p ksee-cli` runs native
   reconstruction, view, program-language, source/text, chart-window, packing,
   and CLI tests.
2. The site is built, as `ksee build` builds it: the tests are in its module.
3. Headless Chrome runs the kernel suite (`ksee-host`): it boots Linux,
   exercises recorded syscalls, and checks stacks, lines, memory history,
   origins, tasks, and timings; then the lessons suite, every lesson and try
   (the module's memory after each).
4. With `--ui`, it also runs the page's suites through the DOM: read (the
   main one, in named phases: made, links and steps, timeline, tip, window,
   editor, panels, program), url (a changed program from the URL), tasks
   (the pipe lesson's task columns, the idle task clicked into), disk (the
   durable lesson's fsync in the Disk tab), responsive (a phone) and tour
   (every lesson as a reader goes, the page's memory after each). Test
   names can also be given directly for focused runs, such as
   `./ksee.sh test read`.

Where a test goes: what needs no browser, a native test in the crate that
has the code (ksee-core's, on made-up records and memory); what the kernel
does, the kernel suite (`ksee-host/src/test/`, through the Machine, no page);
what the page shows, a page suite (`ksee-page/src/test/ui.rs`), each check a
named step that says what it saw when it fails. The page suites wait for the
page to be drawn (`settle`: two animation frames) or for a condition
(`wait`), never a fixed time; the whole run, native and browser, takes
under a minute.

CI (one job: `./ksee.sh build`, then `./ksee.sh test --built --ui`) caches
the kernel's products, by what builds them (the Makefile, the patch, the
port, the plugin): the split kernel (`target/vmlinux`, `vmlinux.debug`), its
UAPI headers and `gen_init_cpio` (`target/linux/usr/`), and the sources the
page shows (`target/site/src/`), some 80 MB. Restored, the build uses them as
they are (`KERNEL_BUILT=true`: the Makefile asks no kbuild, and needs no
Linux tree); else it builds the kernel and saves them, before the tests
(kept even when they fail).

The browser is Chrome's headless shell at the Makefile's `CHROME_VERSION`,
fetched into `target/chrome/` by `make chrome`, which `ksee test` runs.

The CLI serves the test site on a temporary local port and reads the
shell's console on its stderr: each browser suite says `ksee test: ok` or
`fail` and its log there, within five minutes, and an error nothing caught
fails it at once, whether the page's code ran or not.

For a focused native run:

```sh
cargo test --locked -p ksee-core
cargo test --locked -p ksee-cli
```

## Linux patch workflow

The port is `arch/wasm/` (the architecture: its own files) and `linux.patch`
(its few edits to Linux's own files). The first build creates `linux/` from
the pinned stable tag, applies the patch once, and links `linux/arch/wasm` to
`arch/wasm/`. The port is laid out as an architecture is: `kernel/` (entry,
interrupts, the switch, time, signals, the trace ring), `mm/`, `lib/`,
`include/asm/`, `configs/`; and, as UML's `arch/um/drivers/`, `drivers/`: its
devices, the console and the disk, the host's. To change the port:

1. Edit `arch/wasm/` in the repository (`linux/arch/wasm` is a link to it); change
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
