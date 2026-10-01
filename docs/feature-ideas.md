# Feature ideas: what the page could show for free

For the learner, at low code cost. Method: inventory `view::*` fields the
page never reads (grep in `crates/ksee-page/src`, outside selftest/uitest),
against what the recording already computes. Builds on
`docs/info-proposals.md` (its 1–5: `Line.tests`, `Line.ret`, `switch_at`,
`Place.sealed`, sleep/wake pairing) — none of those are re-proposed here.
Every idea below reuses data the recording already has; most are page-only
diffs of 5–15 lines. Mocks are text, on real lessons.

## Ranked

### 1. Say which time round a loop this is (Transport, `Line.run`)

A block runs again in a call only in a loop, so `stack::lines` counts the
turn (`LineStep.run` → `Line.run`; 0 if not counted). The page never reads
`.run`: stepping through a copy loop, every turn looks like the same line,
and the learner cannot tell the first pass from the third.

Mock (read lesson, cursor on `filemap_read`'s copy line, third turn):

```text
prog(19) · filemap_read fs/read_write.c:412 · in read · 3rd time here
```

Transport only: append `· {ordinal} time here` when `here.run > 1`
(1st never shown: most lines run once). No core work. ~8 lines with the
ordinal. Prototyped in this stack (see below).

### 2. Let a task's row go to where it last ran (`TaskAt.last`)

`View::tasks(i)` gives each task its `last` record by then (None: never ran
yet). The Tasks tab shows `prog(20) sleeping in anon_pipe_read` but the name
goes nowhere (it only folds the task_struct). The recording knows exactly
where that task was last seen.

Mock (pipe lesson, cursor in the writer; the reader's row):

```text
prog(20)  sleeping in anon_pipe_read   [click: go to its blocked read]
```

Page-only: make the task name a link calling `ui.go_record(t.last)` when
`Some`. ~6 lines in `panels.rs` Tasks. High value in the two-process
lessons (pipe, futex, join), where the learner chases the other side.

### 3. Name the writer's line in Objects (`Run.at`)

Each member change keeps its runs, and each run keeps where it was written
(`Run.at: Option<Line>` — func/file/line). The Objects box shows the
function (`td.ftype`) and its title tooltip has `in f (file:line)`, but the
row itself never names the line, and a click goes to `r.last` (the write),
not the line that wrote it.

Mock (read lesson, `ksys_read` objects):

```text
f_pos   0 → 18   vfs_read fs/read_write.c:412   [click: go to that line]
```

Page-only: render `r.at`'s `file:line` in the row (today only `func`), keep
the click. ~10 lines in `ObjectBox`. Turns "what changed" into "where it
changed".

### 4. Say where inlined code was inlined from (`Line.site`)

Inlined frames show as `current · inlined`, at the inner line, with no hint
which outer line pulled them in. `Line.site` is exactly that: the outermost
inline site's line. The chain (`View::chain`) is there too for the full
path, but the one line is what orients.

Mock (read lesson, stack at `fdget_pos` inlined into `ksys_read`):

```text
fdget_pos  fs/read_write.c:33  · inlined at ksys_read:710
```

Page-only: append `· inlined at {func}:{site}` when the shown line's `site`
is `Some` (~5 lines in `stack.rs` Stack frame row). Kills the "why am I in
code nobody called" moment around `current`, `fdget_pos`, `SYSCALL_DEFINE`.

### 5. Delete the page's run-folding; use `Chart::bar_runs` / `task_runs`

`Bars` (~12 lines) and `Strip` (~8 lines) each fold columns into runs by
hand. Core already computes both, tested on made-up charts
(`view/tests.rs`: `a_chart_folds_into_runs`): `Chart::bar_runs()` and
`Chart::task_runs()`. The page never calls either.

No visual change; the diff deletes ~20 lines and calls the two methods.
That is the simplification priority in executable form, and it unblocks
later chart work (one folding rule, not three). Owner: page-chart.

### 6. Say what a call returned on the line that made it (`Line.called`)

`Line.called` (func, value, Return record) is read only in Transport, and
only *after* stepping past the call (`c.at <= here.at`). The source line
that made the call shows nothing at the moment the learner wonders what it
gave back.

Mock (pipe lesson, cursor on the writer's `anon_pipe_write` call line,
after its return is known):

```text
written = anon_pipe_write(fds[1], "hello\n", 6);   → 6
```

Page-only dim suffix on the cursor line when `here.called` is `Some` and
known (`c.at <= here.at`, same guard Transport uses). ~6 lines in
`source.rs` Code. Distinct from info-proposal 2 (`Line.ret`: the callee's
last line says what it returns); this is the caller's line saying what it
got. Doing 1 + 6 together is one small diff: "the cursor line says which
turn and what the call gave".

### 7. Say where a value was copied from, in one line (`origin`, first hop)

`View::origin` (last write, back through copies) is read only as a hover
tip on Stack values (up to 8 hops). The read lesson's whole point — bytes
from the page cache into `buf` — never reads as a sentence. The first hop
is usually the sentence.

Mock (read lesson, Stack, `buf` opened after the read):

```text
buf = "hello\n…"  ← copied from 0x… (page cache, written at filemap_read …:412)
```

Page-only: under the values line (or the hover's first line promoted),
`← copied from {addr:#x}` plus `Run`-style place of `way[1]` when there is
one. ~12 lines in `stack.rs`/`source.rs`. Full chain stays in the hover;
this shows only the first hop, so it never sprawls.

### 8. Admit where old bytes were not kept (bulk stores, `old: None`)

Bulk writes past 4096 bytes log where and how much, not what
(`replay.md`; Trace says `store 0x… [8192 bytes] (old contents not kept)`).
Source notes (`Note.short`: `f_pos: 0 → 26`) never carry the caveat, so
stepping back over a big write silently shows today's bytes as yesterday's.

Mock (write lesson, a big copy's line note):

```text
buf: … → …  (earlier bytes not kept; going back shows them as now)
```

Page-only: when any write behind a note has `old: None`, append the short
parenthetical (~8 lines in `source.rs` `ran`, threading one bool out of
`View::notes`' writes — or cheaper: check `View::raw` range once). Honesty
at the one place the learner could otherwise misread the past.

## What was checked and left out

- `At.value`, `SyscallSummary.ret`, `Note.short/full`, `coverage` counts:
  already shown (bar tips, program `= …`, source notes, `ran N×` hover).
- `TaskFields`, `task()` pid/comm-as-then, `definition`/`member_definition`:
  already shown (Tasks open rows, `comm(pid)` everywhere, Ctrl+click).
- `Line.nth`/`at`, `Frame.first`, `Node.depth/under/via`, `Diff.to`:
  plumbing for navigation already wired (`go_record`, object links); naming
  them adds chrome, not answers.
- A member-typed `member_definition(ty, member)` on hover: needs the
  enclosing struct at the cursor — real work, not free. Deferred.

## Prototype (this stack)

- Transport shows the loop turn when `Line.run > 1`: `· 2nd time here`
  (`crates/ksee-page/src/chart.rs`, page-only, ~10 lines). Core already
  computes and tests `run`; no signature changes.
