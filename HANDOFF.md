# Handoff

State of kSEE as of 3a1da43 (branch `dioxus`), for the next agent. Read
AGENTS.md and docs/design.md first; this file is what they don't say.
Delete it once read and acted on.

## The working tree

- Repo: /Volumes/Code/kSEE (git), branch `dioxus`. The main tree is clean
  and green: `CHROME="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" ./ksee.sh test --ui`
  passes (kernel, read, back, pipe, phone).
- `origin/dioxus` is behind local; three pushes happened without the user
  asking. **Never push** (the user hasn't asked for any).
- linux/ in the main tree is built and matches linux.patch. Kernel work
  needs a case-sensitive volume: /Volumes/Code (not /tmp).
- Stale worktrees from earlier agents are still registered (`git worktree
  list`: /tmp/ksee-*, /Volumes/Code/kSEE-*). None holds work that isn't on
  `dioxus` except /tmp/ksee-orch (an old linux.patch edit, superseded).
  They can be removed (`git worktree remove --force …`, then delete their
  branches).

## How to work here

- Build and test: `./ksee.sh build`, `./ksee.sh test --ui` (needs $CHROME).
  Kernel tests' details (which check failed) go to the browser console, not
  the terminal: when `kernel: FAIL`, serve target/test/site and run
  `/tmp/ksee-pw/console.py http://127.0.0.1:PORT/#test=kernel 200` to see
  the `FAIL:` lines.
- `./ksee.sh trace LESSON`: the lesson as the page shows it, a line per →
  step (where, the line, its values, its note, the Stack). Good for
  reviewing stepping. Slow for big lessons (it steps the real page; the
  ext4 ones time out at 2 min; output also seems capped near 1000 lines).
- Playwright for looking at and driving the page (dev-only, not in the
  repo): /tmp/ksee-pw (a venv). `/tmp/ksee-pw/shot.py URL OUT.png
  [--size=WxH] [--do=key:ArrowRight|click:CSS|hover:CSS|move:X,Y|drag:…|wheel:X,Y,DY] [--js=EXPR]`
  waits for the boot. Serve target/site with `python3 -m http.server`.
- Commit small, in the repo's style (`git log --oneline`), one change each,
  with what it removes. Run the full `test --ui` before committing anything
  that touches the page or the core; I once committed a red tree by
  chaining `;` instead of `&&`.
- Sub-agents: they get cut off at ~60 minutes, often stall on long builds,
  and three in parallel hung for 40 minutes. Doing work directly was
  faster and safer this session.

## What the user wants

- Simplify above all: delete low-value code, fewer types and fields, no
  hacks, no special cases, no new dependencies (ask first). Report net
  lines.
- Desktop first; the phone layout is not a priority.
- The page should behave like a debugger a learner can trust: ← → step a
  line (Shift: over calls), ↑ ↓ frames; hover the timeline previews,
  click commits; inlined calls behave like calls.

## What changed this session (recent, most relevant)

- One `Moment {call, line, frame}` for the cursor (`Ui::at`) and the
  hover (`Ui::peek`); `Derived::shown` = peek ?? at; all views read it.
- Inlined calls: the line calling one is a stop of its own
  (`LineStep::cut` / `Line::cut`, `View::call_stack(.., cut, ..)`).
- Stepping values are read at the line's own record (`Line::at`), not its
  stretch's first (fixed stale values like `error = 0`).
- Chart: a stretch is as many steps as its lines, a column is (record,
  line) (`Column::nth`); the window is in steps (`Ui::win`, set only via
  `Ui::view`); selecting a call zooms to it; the window follows the
  cursor; the wheel zooms smoothly and pans.
- Source: scrolls only near the edges; the cursor line's values,
  comparisons and returned values at its end (no extra row); sealed
  functions say so in the bar.
- Notes and Objects share one data path: `View::things` + `View::change`.
- Removed: Trace tab, `ksee type`, `ksee shot`, CDP, Alt-step-out,
  `View::at`, `switch_at`, docs/related, the proposal docs (now
  docs/ideas.md).
- Top bar: kSEE, Lessons, ◀ ▶, the chapter crumb, a "keys" hint, one theme
  button (Auto → Light → Dark).

## Open, roughly by value

1. **Decisions for the user** (ask before doing): drop editor completions
   (~120 lines), pane resizing and saved sizes (layout.rs ~108), the task
   strip under the chart (~25), the find box (~40); fold the runtime's
   `Kernel` (21 Cell/RefCell fields) into one `RefCell<State>`.
2. **Code has grown** (+~600 lines net in crates/ since the session began,
   mostly tests and requested features). Keep looking for structs with
   fields few read (view/mod.rs, stack.rs `LineStep`, runtime `Kernel`),
   one-caller helpers, and duplicated "where" types (Line, Frame, Place,
   Write/Run's `at: Option<Line>`).
3. **Stepping review**: run `ksee trace` on small lessons and read it. Known
   oddities: a declaration line revisited after later lines (compiler
   order, e.g. sys.c 2542 → 2538); comparisons print unsigned hex for
   signed values (`0xffffffda == 0xffffffda` for -ENOSYS: icmp eq has no
   sign).
4. **Plugin** (plugin/ksee-trace.cpp, 715 lines): a variable whose
   `#dbg_value` has a DIExpression (e.g. a conversion) is not recorded
   (`!R.getExpression()->getNumElements()`), so its value can be missing.
   Any plugin change rebuilds the kernel (~5 min) and must pass
   `./ksee.sh test`.
5. **Tests**: uitest.rs's `read()` is one long scenario; the plan was
   short named smoke tests plus moving pure logic into ksee-core with
   native tests.
6. docs/ideas.md: shown-data ideas (switch handoff, task row → where it
   last ran, Objects naming the writer's line, …) and removal candidates.
