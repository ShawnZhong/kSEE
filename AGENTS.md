# kSEE, for agents

Read [docs/design.md](docs/design.md) first (the goal, and the choices that
shape everything), then the doc for the part you touch ([docs/](docs/README.md)).

- **Simplicity.** Prefer deleting to adding; when a change sprawls, step
  back to the goal. A feature earns its place by what a user gets from it:
  a toggle, a fold, a shortcut or a view that repeats another is a
  candidate to remove.
- **Rules, not cases.** No special case for one syscall, function, struct
  or example: find the rule the data already says (the types, the
  recording, the debug info), or ask. Kernel names belong to the kernel,
  `ksee pack` and ksee-core's arch.rs, never to web/.
- **Nothing is fixed.** Every decision, earlier ones included (this file's
  too), can be revisited: propose what seems right, with the reason. There
  is no compatibility to keep: nothing outside this repository depends on
  its formats or APIs, so change them all together and drop the old. Ask
  before a new mode, language or dependency.
- **Rust does the work** in ksee-core, unit-tested on made-up records; web/
  only lays out what `Machine` returns.
- **linux/** (submodule, branch wasm, on the stable tree's v7.2.7) may
  change in small commits: in arch/wasm where possible, elsewhere only
  what the port needs. Commit linux.patch with it (the port as a patch
  to the release: `ksee build` writes it).
- **Before you finish:** `./ksee.sh test` (`--ui` with `$CHROME` runs the
  page too; the tests check what the page shows). Commit small. Use the page
  as a user would (`deno run -A tests/ui.js OUT.png --eval=…` for a
  screenshot); measure before claiming a cost. The docs and README describe
  the current state: change them with the code.
