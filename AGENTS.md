# kSEE, for agents

Read [docs/design.md](docs/design.md) first: the goal and the choices that
shape everything; then the doc for the part you touch ([docs/](docs/README.md)).

- Simplicity is the standing request: prefer deleting to adding; when a
  change sprawls, step back to the goal. No ad hoc checks or special
  cases (for one syscall, one function, one struct, one example): find the
  rule the data already says (the types, the recording, the debug info),
  or ask. Ask before a new mode, language or
  dependency. Earlier decisions are not fences: propose again what seems
  right, with the reason.
- Rust does the work, in ksee-core (unit-tested on made-up records); web/
  only lays out what `Machine` returns. Kernel names belong to the kernel,
  `ksee pack` and ksee-core's arch.rs, not to web/.
- linux/ (submodule, branch wasm) may change, in small commits: in arch/wasm
  where possible, outside it only what the port needs.
- `./ksee.sh test` before you finish (`--ui` with `$CHROME` runs the page too).
- The docs and README describe the current state: change them with the code.
