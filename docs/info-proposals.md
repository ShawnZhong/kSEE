# What the page should show at each moment

A ranked list of gaps between what the recording already has and what the
page shows a learner, one per moment kind: syscall entry, inside a
function, a store, a return, a context switch, sleep/wake. Mocks are text,
on real lessons. Method: an inventory of `trace::Record` → `view::*` →
`ksee-page` components, verified by grep (no `.tests`, `.ret`, `.run`,
`.site`, or `sealed` read anywhere in `crates/ksee-page/src` outside
selftest/uitest). The page builds and runs (`ksee shot` PASS, no
exceptions); visual inspection was not possible here, so every claim below
names the struct field and the component that does or does not read it.

## Ranked proposals

### 1. Show why a branch went, at the cursor (Compare → `Line.tests`)

The recording has every deciding comparison (`Record::Compare`: operands,
predicate, whether it held). Core computes them per line (`View::line` →
`Line.tests`, e.g. `"18 >= 0 ✓"`). The page never reads `Line.tests`:
no page view shows a comparison.
The design goal's word is *why*: at an `if`, the learner should see the
values the branch went by, next to the line.

Mock (read lesson, cursor on `vfs_read`'s read-access check):

```text
if (unlikely(!(file->f_mode & FMODE_READ)))        [count=18 ≥ 0 ✓ → took else]
```

Rendered as a dim suffix on the cursor line, or appended to the transport's
`where` line (`… · vfs_read fs/read_write.c:412 · count ≥ 0 ✓`). One or two
tests; more fold into `+2`. No core work: `Line.tests` is computed already.
Page-only diff, for page-lesson (Transport or source values line).

### 2. Say what a function returns, where it returns it (`Line.ret`)

A call's last line before it returns knows the value (`LineStep.ret` →
`Line.ret`; selftest asserts `vfs_read`'s last line says 26). The transport
shows `· vfs_read returned 26` only *after* stepping past the call
(`Line.called`). At the return moment itself — cursor on the function's
last line — nothing says `= 26, about to return`. Learners stepping into a
function never see its result at the place that produced it.

Mock (read lesson, cursor on `ksys_read`'s closing line):

```text
}                                   → 26
```

Same treatment as proposal 1 (dim suffix on the cursor line when
`Line.ret` is `Some`). No core work. Page-only, for page-lesson. Doing 1
and 2 together is one small diff: "the cursor line says what it tested and
what it returns".

### 3. Name the handoff at a context switch (prev → next) — prototyped

At a switch the Stack and the strip change, and the transport names the new
task, but nothing says who left and who arrived. The recording knows both:
the task changes when `__switch_to(prev, next)` returns, and the call's
arguments name the parties. New core surface, additive only:
`View::switch_at(i) -> Option<(Task, Task)>` (prev, next), with tests on
made-up records — see "Prototype" below.

Mock (pipe lesson, cursor at the switch into the writer):

```text
prog(19) → prog(20) · in __switch_to          (transport, replacing the bare task name there)
```

Page half (for page-lesson): in `Transport`, prefer the handoff line when
`switch_at` returns `Some`. About five lines.

### 4. Say sealed functions are sealed (`Place.sealed`)

`recording.md` promises "the source view says so" for sealed functions
(call and returns only: `crc32c`, `memcmp`, ~130 of them). The page never
reads `Place.sealed`, so a learner stepping into one sees a call with no
lines and no explanation. One-line fix in the source bar or the frame row:

```text
memcmp · sealed: only its call and return are recorded
```

No core work (`Place.sealed` exists). Page-only, for page-lesson. Ranked
below 1–3 only because sealed functions are rare in the early lessons.

### 5. Say why a task sleeps, and what will wake it, at the moment

The Tasks tab already shows sleeping + where (`TaskAt.state`,
`syscall`/`func`: "sleeping … in anon_pipe_read"). What it does not say is
the pairing the pipe lesson's prose carries: *waiting for what, woken by
which later line*. E.g. at the child's blocked read: "waiting in
`anon_pipe_read` (read fds[0]); woken during line 7". The wake side needs a
cross-syscall read (the writer's span, `try_to_wake_up`), so this is the
only proposal needing real new core work — ranked last among the gaps.
Suggested shape, not prototyped: extend the sleeping row's `place` string
with the wait point it already has, and link the waker's line where the
recording names one. Defer until 1–4 land.

## Already covered — no action (the brief's examples, verified)

- **Store old values.** Shown two ways: source notes (`f_pos: 0 → 26`,
  `Note.short`, full text as hover, click goes to the write); Stack value
  hover ("was …", `View::origin` → `Write.old`). Nothing to add.
- **Syscall entry arguments.** The entry frame carries the syscall's name
  and typed, constant-named arguments (`Frame.syscall`, `syscall_args`);
  chart bar tips repeat them with the result. Covered.
- **Task states.** Tasks tab shows running/runnable/sleeping/exited/later
  with per-state explanations (`state()` in panels.rs). Covered.
- **What a call did.** Objects tab (by object, by task, changed vs.
  changed-back) plus per-line notes. Covered; its density is the product,
  not clutter.

## Clutter: almost none

Each pane earns its place against the design doc's layout. One minor
candidate, not worth a removal diff on its own:

- The transport's `· in {call}` repeats the lesson's selection, but it is
  the only orientation line when arriving via find or chart click. Keep.

No removals proposed.

## Prototype (this doc's commit stack)

- `View::switch_at` in `crates/ksee-core/src/view` (+ tests): proposal 3's
  core half. Additive, no signature changes; `cargo test -p ksee-core`
  passes.
- Proposals 1, 2, 4 need no core changes (`Line.tests`, `Line.ret`,
  `Place.sealed` are computed and tested today); their page diffs are
  offered to page-lesson as five-to-ten-line sketches, not applied here.
- Proposal 5 is deferred, with the suggested shape above.
