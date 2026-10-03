# kSEE documentation

Use this page as the documentation map. The root [README](../README.md) gives a
short system overview and a minimal way to run kSEE.

## Start here

- [Getting started](getting-started.md): prerequisites, setup, build, serve,
  browser support, testing, and troubleshooting.
- [User guide](user-guide.md): lessons, the timeline, source, stacks, tasks,
  objects, console output, and navigation.
- [Lesson authoring](lesson-authoring.md): lesson JSON, recording links, tries,
  task steps, expressions, course ordering, and fixtures.
- [Development](development.md): repository layout, build pipeline, commands,
  generated files, page internals, tests, and the Linux patch workflow.

A newcomer who wants to understand the implementation should read
[the design](design.md), then [the recording](recording.md),
[replay](replay.md), and [the runtime](runtime.md). Read the kernel port when
you need the WebAssembly execution model or Linux-specific details.

## Architecture

- [Design](design.md): goals, boundaries, system flow, rationale, and limits.
- [Kernel](kernel.md): Linux `arch/wasm`, task stacks, virtual time, programs,
  runners, storage, and the host interface.
- [Recording](recording.md): LLVM instrumentation, runtime records, the trace
  ring, and packed kernel metadata.
- [Replay](replay.md): reconstruction of tasks, stacks, source lines, memory,
  origins, objects, and chart data.
- [Runtime](runtime.md): shared memory, kernel boot, JSPI scheduling, the runner
  protocol, snapshots, and the page/runtime boundary.

## Historical notes

These pages preserve earlier exploration. They are not current roadmaps or
authoritative descriptions of kSEE:

- [Ideas archive](archive/ideas.md): historical open and obsolete ideas.
- [Research archive](archive/research.md): preliminary survey notes and project
  proposals from September 2026.
