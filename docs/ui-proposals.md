# UI proposals: a full-width top bar (and what stays where)

For page-lesson (the Header, the lessons menu, the theme toggle) and
page-chart (the timeline and the transport, which do not move). CSS and
docs only: no Rust, no new dependencies. The CSS half lives in
`web/index.html` (the `.topbar` rules and the one `light-dark()` palette
with the `data-theme` forcing rules); the DOM half is offered as exact
owner can land it in one small diff.

## Why a top bar

Today the page's global chrome lives in `.sidehead`, inside the lesson
column (`lesson.rs` `Header`: the `kSEE` brand, `☰ Lessons`, the `◀ ▶`
lesson pager). That steals ~340px-wide column space for controls that
are not the lesson, and leaves nowhere to put page-global controls
(the theme toggle below). A full-width bar above `main.page` fixes
both, and matches the design doc's layout: one global strip, then the
lesson | work split.

## What changes

- A new `header.topbar`: the first child of `#root` (which is already
  `display: flex; flex-direction: column`), full width, `~34px` tall,
  `background: var(--panel); border-bottom: 1px solid var(--line)`.
- Moving into it, unchanged in behavior: the `b` brand `kSEE`
  (`title: "watch the Linux kernel run"`), the `☰ Lessons`
  `button.contents`, and the `◀ ▶` lesson pager buttons. Owner:
  page-lesson (`lesson.rs` `Header`).
- New in it, at the right (`margin-left: auto`): the theme toggle (next
  section). Owner: page-lesson.
- `main.page` becomes the second child of `#root`, `flex: 1`, otherwise
  untouched. The `.side` column keeps its lesson, minus its header.
- The lessons menu (`nav.menu`) moves with the header: it anchors under
  the bar, not the column — `.topbar { position: relative }`,
  `.topbar .menu { position: absolute; top: 100%; left: 0;
  width: min(340px, 92vw); bottom: auto; max-height: 70dvh; ... }`,
  keeping today's item/chapter/tick styles. Owner: page-lesson. (Today
  `.menu` is `top: 33px; bottom: 0` inside `.side`; that rule stays for
  the old DOM until the move lands.)
- The transport (`div.transport`: `◀ ▶ ▲ ▼` plus `.where`) STAYS under
  the timeline in `.history`. Owner: page-chart, no change asked. It
  acts on the cursor, so it belongs next to the chart, not in global
  chrome; the top bar keeps only lesson-level nav and the theme.

## The Auto / Light / Dark toggle

- The control: `div.theme` with `role="group"`, three native `<button>`s
  labelled `Auto`, `Light`, `Dark`; the active one gets class `on`
  (the same pattern as `.contents.on` and `.tab.on`). `title`s:
  `"follow the system"`, `"light"`, `"dark"`. Native buttons, so tab
  focus and `Space`/`Enter` work; add `:focus-visible` outlines with the
  rest of the buttons (in this diff's CSS).
- The state: `localStorage` key `ksee.theme`, one of
  `"auto" | "light" | "dark"`, defaulting to auto (the key absent means
  auto, like `ksee.layout`'s default). Applied as
  `document.documentElement.dataset.theme`: `"light"` sets
  `data-theme="light"`, `"dark"` sets `data-theme="dark"`, auto removes
  the attribute. Owner: page-lesson, about twenty lines next to the
  header (read once at mount, set on click).
- The CSS contract, provided by this diff, so the toggle needs no
  further CSS: one set of variables in `:root` via `light-dark(light,
  dark)` (light value first), with `color-scheme: light dark`; Auto is
  the attribute absent (the system decides). Explicit
  `:root[data-theme="light"] { color-scheme: light }` and
  `:root[data-theme="dark"] { color-scheme: dark }` force each mode —
  both directions work regardless of the OS setting, which the old
  `@media (prefers-color-scheme: dark)` block could never do.
- Nothing else reads the theme: charts, source colors, and tabs all go
  through the `--*` variables already.

## What does not change (test and selector notes)

- `crates/ksee-page/src/uitest.rs:269` clicks
  `all(".sidehead button").last()` for the next lesson. Two ways to
  stay green, page-lesson's choice: (a) keep class `sidehead` on the
  moved header element (CSS styles `.topbar .sidehead` the same), zero
  test churn; or (b) rename to `topbar` and update that one selector in
  the same diff. The CSS in this diff styles `.topbar` fully and leaves
  every `.sidehead` rule in place, so the page looks right either way
  during the handoff.
- `.transport`, `.find`, `.tabs`, `.cell textarea`,
  `.ptitle a`, `.explain a` selectors are untouched by this proposal.
- Phone layout (`@media (max-width: 800px)`): the top bar keeps brand +
  `☰` + theme on one row; the lesson pager stays (small buttons, as
  today). The menu's `width: min(340px, 92vw)` keeps it on screen.

## Critique of the current page (screenshots, 1280x800 and smaller)

Taken with `/tmp/ksee-pw/shot.py` against the local build; paths in
`/tmp/ksee-design2-shots/`. One image viewed per check; the rest judged
by DOM metrics (`--js`) plus the shots as artifacts.

- `01-baseline.png` (1280x800): global chrome sits in the 340px lesson
  column while the work area has no header at all — the motive for the
  top bar above. Otherwise the three-pane split reads well.
- `02-menu.png` (960x600, after clicking `☰ Lessons`): the menu fills
  the whole lesson column (`top: 33px; bottom: 0`), hiding the lesson
  it navigates from. Under the top bar it becomes a dropdown
  (`top: 100%`, bounded height), which also fixes this.
- `03-narrow.png` (760x800): the phone stacking works, but the lesson
  pager and the theme (once added) compete for one row — hence the
  top-bar responsive rule above (brand + `☰` + theme only).
- `04-dark.png` (960x600, `data-theme="dark"` forced): proves the new
  unconditional dark override; before this diff, forcing dark with the
  OS in light mode was impossible (the only dark rule lives inside the
  media query).
- `05-after.png` (1280x800, same framing as 01, with this diff's CSS):
  pixel-equivalent layout — the new selectors match no current DOM, so
  the polish changes nothing until page-lesson/page-chart adopt the
  classes. (Verified by serving the edited `web/index.html` with the
  same artifacts on `:8125`.)
