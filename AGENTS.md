# kSEE, for agents

Read [docs/design.md](docs/design.md) first: the goal, and the choices that
shape everything (real Linux, the compiler records, replay only, Rust does
the work and JS lays it out). Then the doc for the part you touch
([docs/](docs/README.md)).

- Simplicity is the standing request here: prefer deleting to adding,
  and when a change sprawls, step back to the goal. No new modes, languages or dependencies
  without asking. Kernel names live in the kernel, `ksee pack` and
  crates/ksee-core/src/arch.rs, not in web/.
- Rust does the work: anything that needs no browser goes in ksee-core (and
  gets a unit test on made-up records); the page (web/, plain ES modules)
  only lays out what `Machine` returns.
- Comments say what and why in prose, as the code does now; keep the style.
- Test: `./ksee.sh test` (core unit tests, then tests/web.js under Deno);
  `./ksee.sh test --ui` with `$CHROME` set also runs the page
  (tests/ui/*.js). Run at least `./ksee.sh test` before you finish; the
  build is incremental.
- Docs and README describe the current state: update them with the code.
