# kSEE, for agents

Read [docs/design.md](docs/design.md) first (the goal and the choices that
shape everything), then the doc for the part you touch ([docs/](docs/README.md)).
Use [docs/development.md](docs/development.md) for commands, tests, repository
layout, generated files, and the Linux patch workflow.

- **Simplicity.** Prefer deleting to adding; when a change sprawls, step
  back to the goal. A feature earns its place by what a user gets from it:
  a toggle, a fold, a shortcut or a view that repeats another is a
  candidate to remove.
- **Rules, not cases.** No special case for one syscall, function, struct
  or example: find the rule the data already says (the types, the
  recording, the debug info), or ask. Kernel names belong to the kernel,
  the build's pack and ksee-core's arch.rs, never to the page.
- **Nothing is fixed.** Every decision, earlier ones included (this file's
  too), can be revisited, and you are free to change anything: a struct,
  a module, a format, the recording, the plugin, the page's state.
  Large-scale refactoring is welcome when it makes the code smaller or
  clearer: a type holding fields its users don't need, two paths doing
  one thing, a layer that only passes things on. There is no
  compatibility to keep: nothing outside this repository depends on its
  formats or APIs, so change them all together and drop the old. Ask
  before a new mode, language or dependency.
- **Keep the Rust boundaries clear.** ksee-core owns browser-independent
  decoding, reconstruction, views, and program rules, with native tests on
  made-up records. ksee-host owns the kernel's memory and execution. The
  Dioxus page drives `Machine` directly and owns UI state and presentation;
  it does not maintain a second replay model.
- **The port is arch/wasm/ and linux.patch** (to the stable tree's
  v7.2.7: the build fetches it into linux/, applies the patch and copies
  arch/wasm/ in). Change arch/wasm/ (in the repository, not linux/)
  where possible; Linux's own files only where the port needs it, in
  linux/, then `make patch` writes linux.patch from it (never the build):
  commit that.
- **Commit promptly.** Once a coherent change passes its focused checks,
  commit it instead of leaving finished work mixed with later exploration.
- **Before you finish:** `./ksee.sh test` (`--ui` runs the
  page too; the tests check what the page shows). Commit small. The docs and
  README describe the current state: change them with the code.
