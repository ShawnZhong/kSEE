# TODO (handoff, branch `dioxus`)

Read AGENTS.md and docs/design.md first. The page moved from JS to Rust
(crates/ksee-page, Dioxus 0.7, web only, built by cargo + wasm-bindgen, no
`dx`). Only `Machine.start` is exported to JS; web/index.html is the frame
and holds all the CSS. Nothing is merged to master or pushed.

## The user's verdict, first

**The old JS page (live at https://shawnzhong.com/kSEE, the code on
`master`) looks better than the Rust one.** Do not go back to JS ("no do
not go back to javascript", "have a better rust version"): make the Rust
page match or beat it. Open both side by side, take screenshots
(`deno run -A tests/ui.js OUT.png --eval=…`, with `CHROME` set), and copy
the old look: its spacing, type sizes, colours, panel proportions, the
lesson cells, the chart. `git show master:web/ui.css` and
`master:web/*.js` have the old styles and layout. The user also said the
UI was too large: sizes were cut (13px base, 12px mono, a 36px bar), so
check that against the old page as well.

## State (uncommitted at handoff, now committed as WIP)

Done in Rust:
- layout.rs: draggable splits (left, chart, right), saved in localStorage
  `ksee.sizes`.
- state.rs: `Ui` signals and `Derived` memos (window, moment, stack,
  shown).
- chart.rs: the overview, wheel zoom and drag pan, a tooltip that peeks
  the stack, find with a count and Enter / Shift+Enter, the legend, rows
  that fit the height.
- stack.rs: ← → (Shift: over, Alt: out), ↑ ↓ for the frame, stepping on
  past the syscall, a current-task link, and on a value's hover who wrote
  it.
- highlight.rs: our own C highlighter (syntect and tree-sitter are too
  heavy; no mature Dioxus editor or virtual-list crate exists).
- source.rs: virtual scrolling over the whole file; hover values;
  Ctrl+click goes to the definition; the cursor line's values; notes; a
  line number's click runs to that line; run counts.
- lesson.rs: the "☰ Lessons" menu with ticks, ✕ on a cell, "↺ back to
  it", the ends.
- editor.rs: a textarea with completions and the signature hint.

Just fixed: the global `.now` CSS rule (the chart's marker) made the
source's cursor line `position: absolute`, so it was hidden. It is now
`.chart .now`. Verify with a screenshot of `#ex=pipe`.

## Still missing (bring back, in Rust)

1. **Lanes** (pipe and thread lessons): the grid columns are too narrow
   and lines wrap badly. Give each cell an edge coloured by its task's hue
   and a runner chip, in one vertical order, with simple wait markers (or
   copy what master's page did).
2. **Objects tab in full** (panels.rs is the simple version; see master's
   web/objects.js):
   - the head summary ("read by prog(19) changed 3 objects…");
   - boxes with label, what, within, and a link to the global's
     declaration;
   - changed groups first;
   - per-member runs (before → after), a click going to
     `go_record(run.last)`;
   - pointer links;
   - the tree via `under`/`via`;
   - other tasks' sections;
   - writes not yet made shown gray.
3. **Tasks:** a tree by `up`, the running task marked and opened.
4. **Trace tab** (a placeholder now): restore `View::raw` and `var_at`
   from master's ksee-core, show the records around the moment, and a
   click goes to one.
5. **UI tests** (tests/ui/*.js) need updating for the new DOM: the header
   has no `select` and the cells are textareas. Run
   `./ksee.sh test --ui` with
   `CHROME=/home/ubuntu/.cache/ms-playwright/chromium_headless_shell-1243/chrome-headless-shell-linux-arm64/chrome-headless-shell`.
   The kernel checks are already Rust (crates/ksee-page `selftest`
   feature, driven by tests/web.js).
6. **Docs:** README's page section and docs/runtime.md's module table
   (layout.rs, highlight.rs). Commit small on `dioxus`.

## Notes

- Measured:
  - Dioxus adds about 140 KB gzipped; the module is 463 KB;
  - stepping takes about 16 ms; a lesson switch renders in 3–6 ms.
- `with_context` needs `Send`, so the Machine lives in a thread-local
  (`MACHINE`).
- A key is only allowed on the first node of an rsx block; use
  `Fragment { key }`.
- `use_resource(use_reactive!(|(p,)| …))` refetches as `p` changes; keys
  on a single child do not remount it.
