# Deletion menu (value audit)

## Score — net lines removed since f190b53 (excl. linux.patch, Cargo.lock)

- Committed (67cfe65) vs f190b53: +2889 / −1960 → net **+929 added**
  (1960 lines deleted, 2889 added).
- Working state vs f190b53, tracked files: same as committed (main tree
  clean; only this file's update on top).
- This file, docs/deletion-menu.md: +120.
- Net removed: **−929** (the tree grew 929 lines since the base; deletions
  are outpacing additions commit by commit — see the deletion log below).

Method: `git diff f190b53 --numstat -- . ':!linux.patch' ':!Cargo.lock'`
(insertions vs deletions; net removed = deletions − insertions).
`linux/` and `target/` are untracked/ignored, so they never count.
Stamped 2026-10-01 ~10:40 UTC at 67cfe65 (main clean). Re-stamp with the
same command after the tree settles.

## 1. Inventory — line cost at HEAD

ksee-core: ~7060 lines, 26 files. Biggest: types.rs 629, view/mod.rs 544,
view/tests.rs 540, stack.rs 517, view/stores.rs 452, program/parse.rs 437,
tables.rs 381, program/layout.rs 331, view/frames.rs 317, trace.rs 315,
highlight.rs 314, program/tests.rs 305, recording.rs 304, view/objects.rs 296,
program/eval.rs 224, module.rs 217, mem.rs 156, view/value.rs 136,
arch.rs 125, program/lex.rs 104, program/lower.rs 88, program/complete.rs 61,
text.rs 58, program/names.rs 58, lib.rs 45, program/mod.rs 37.

ksee-page: ~2920 lines, 11 files: selftest.rs 483, lesson.rs 476, chart.rs 411,
uitest.rs 322, source.rs 249, panels.rs 249, state.rs 212, stack.rs 171,
lib.rs 133, layout.rs 108, editor.rs 104.

ksee-runtime: 1476 lines, 8 files: machine.rs 385, sched.rs 326,
programs.rs 291, state.rs 193, imports.rs 169, replay.rs 62, lib.rs 26,
js.rs 24.

ksee-cli: 1595 lines, 9 files: pack/dwarf.rs 543, build.rs 429, serve.rs 170,
pack/image.rs 118, pack/constants.rs 119, pack/mod.rs 54, main.rs 53,
chrome.rs 71, deploy.rs 38.

Rest: plugin/ksee-trace.cpp 711, web/index.html 237 + web/ksee.js 22,
user/init.c 59 + user/prog.c 68 + user/syscall.h 14.
Docs (handbooks): design 118, kernel 190, recording 121, replay 171,
runtime 145, research 57, README 37, info-proposals 132, ui-proposals 111
(~1082); README.md 118, AGENTS.md 28, ksee.sh 10, setup.sh 24.
Background: docs/related/ 2133 (README 51, emulators 301,
hardwarejs-brintos 375, joelseverin-linux-wasm 480, linuxontab 234,
tombl-linux 266, wasmlinux-lkl 426).
Lessons: user/examples/ 30 JSON files, 569 lines total.

Tests inside the above: core view/tests 540 + program/tests 305, page
selftest 483 + uitest 322, cli serve/build tests ~60 — about 1700 lines
(~15% of the tree) that any deletion must keep passing.

## 2. Lesson dependencies

30 lessons in 7 chapters (EXAMPLES in crates/ksee-cli/src/build.rs:353):
Start here (umask, umask-inside); A process (getpid, prctl, exec, thread);
Files, in memory (hello, open-close, dup, read, write, open-fails, create,
dir); Files on a disk, ext4 (ext4-read, ext4-create, ext4-write, durable,
ext4-dir, link, remove); Waiting and waking (sleep, futex, join, pipe,
kill); Requests in memory, io_uring (uring-setup, uring-read, uring-worker);
Your own (yours).

Steps use most: openat 15, mmap2 10, write/clone/futex/read 5, unlinkat 4,
io_uring_setup/getpid 3, umask 2. Every `tries` entry is linked from its
lesson text (no orphan tries — verified by script).

Link targets: ~120 distinct function links (every subsystem gets looked
at, so no core view can go without breaking links). Tab links: `:objects`
8 links in 7 lessons (create, dup, exec, link, prctl, read, umask-inside);
`:console` 1 (hello); `:any` modifier in 5 lessons (durable, getpid, pipe,
sleep, uring-worker); `:stack` 0, `:tasks` 0 (design-core views, just never
linked).

## 3. Ranking — lines saved / value lost

| Rank | Candidate | Saving | Value lost | Route |
|---|---|---|---|---|
| — | ~~Dead `fn read`, cli main.rs:21-23~~ WITHDRAWN | 0 | my grep was scoped to main.rs only; `crate::read` has live callers in build.rs:367, pack/mod.rs:16, pack/constants.rs:44 | RETRACTED — apologies to cli-refactorer-2 |
| — | Stale shot clause, docs/README.md:25-27 | ~2 | zero (shot dropped) | LANDED by owner (also reworded the `ksee pack` half; verified in tree) |
| — | Stale shot mention, info-proposals.md:9 | ~1 | zero (reword) | SENT info-designer |
| J1 | docs/related/, all 2133 lines | ~2133 | pre-port background only; in git history if ever needed | USER DECIDES |
| J2 | Trace tab (View::raw ~50 + Trace comp ~47 + tests) | ~110 | only raw-record surface; comparisons live only here | LANDED 4d93312 (−164: tab, panel, CSS, View::raw + tests, docs mentions). Zero leftover refs (verified by grep). Lesson impact: none — lessons never linked `:trace`; only `:objects` (8), `:console` (1), `:any` (5) |
| J3 | Completions (complete.rs 61 + editor.rs 104 + tests) | ~175 | typing UX, core to Your-own | USER DECIDES |
| J4 | Layout drag/storage (layout.rs 108; storage-kept ~25) | 25–108 | resizable panes / kept sizes | USER DECIDES |
| J5 | Find box (chart.rs ~35) | ~35 | function navigation for devs | USER DECIDES |
| J6 | `ksee deploy` (deploy.rs 38 + arms/docs) | ~50 | publishing to gh-pages | USER DECIDES |
| J7 | Proposals docs (info 138 + ui 111) | ~249 | working notes; fold after tasks 12/13 close | DEFER |
| J8 | umask-inside overlap; yours.json (8) | ~30 | first-look funnel; page-lesson call | USER DECIDES |

NOT candidates (lesson- or design-load-bearing): Objects tab (8 lesson
links), Console tab (program output surface), Stack/Tasks tabs
(design-core), ext4 chapter (7 lessons + disk image), program language
(every lesson), text.rs pieces (lesson renderer), module.rs/js.rs (all
fns have callers — verified).

## 4. Sent to owners (clear deletions)

- cli-refactorer-2: stale shot clause (docs/README.md:25-27) — LANDED by the
  owner (who also reworded the `ksee pack` half; verified in tree). The
  `fn read` item was WRONG — live `crate::read` callers in build.rs:367,
  pack/mod.rs:16, pack/constants.rs:44 — withdrawn with apologies.
  /private/tmp/ksee-cli2 is the owner's active workspace, not stale;
  no action there.
- info-designer: stale shot mention (docs/info-proposals.md:9, reword).

## 5. Judgement calls for the user (morning)

- [ ] J1: delete docs/related/ (~2133 lines)? Reversible via git history.
- [x] J2: Trace tab — LANDED 4d93312 (−164), zero leftover refs.
- [ ] J3: drop completions + editor popup (~175 lines)? Loses typing help.
- [ ] J4: drop layout drag and/or storage-kept sizes (25–108 lines)?
- [ ] J5: drop the chart Find box (~35 lines)?
- [ ] J6: drop `ksee deploy` (~50 lines)? Only if Pages publishing moves out.
- [ ] J7: fold/delete proposals docs after tasks 12/13 close (defer, not now).
- [ ] J8: merge umask/umask-inside; keep yours.json as the Your-own stub?

Own micro-deletions this round: none in code (tree churning under owners;
all code deletions routed). This file is the only addition (~120 lines).
