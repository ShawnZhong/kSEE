# kSEE, for agents

Read [docs/design.md](docs/design.md) first: the goal and the choices that
shape everything; then the doc for the part you touch ([docs/](docs/README.md)).

- Simplicity is the standing request: prefer deleting to adding; when a
  change sprawls, step back to the goal. Ask before a new mode, language or
  dependency.
- Rust does the work, in ksee-core (unit-tested on made-up records); web/
  only lays out what `Machine` returns. Kernel names belong to the kernel,
  `ksee pack` and ksee-core's arch.rs, not to web/.
- `./ksee.sh test` before you finish (`--ui` with `$CHROME` runs the page too).
- The docs and README describe the current state: change them with the code.
