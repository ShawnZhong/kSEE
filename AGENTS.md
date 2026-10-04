# kSEE, for agents

Read [docs/design.md](docs/design.md) first, then the doc for the part you
touch; [docs/development.md](docs/development.md) has the commands.

- **Simplicity.** Prefer deleting to adding; unify what does one thing twice.
- **Rules, not cases.** No special case for one syscall, function or example:
  find the rule the data already says, or ask.
- **Nothing is fixed.** Anything can change, this file included; there is no
  compatibility to keep. Add a dependency when it makes the code simpler.
- **Linux's own files** change in linux/; `make patch` writes linux.patch.
- **Test and commit.** `./ksee.sh test --ui` passes, docs match the code,
  then commit, small and often.
