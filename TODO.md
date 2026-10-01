# TODO (handoff, branch `dioxus`)

Read AGENTS.md and docs/design.md first. The page is Rust
(crates/ksee-page, Dioxus 0.7, web only, built by cargo + wasm-bindgen, no
`dx`). Only `Machine.start` is exported to JS; web/index.html is the frame
and holds all the CSS. Nothing is merged to master or pushed.

## Where it stands

The user found the old JS page (https://shawnzhong.com/kSEE, `master`)
better looking. The Rust page now follows its layout and style:
- the lesson column runs the page's height, with its head (☰ Lessons,
  ◀ ▶) on top;
- the text comes first, then the PROGRAM, with 300px lanes in each task's
  hue;
- the timeline shows the whole recording, with the overview, labelled
  rows, the cursor's calls darker, and the transport under it;
- the Stack sits at the bottom of its tab, with only the chosen frame open;
- Tasks is a tree, Objects is complete, and the Trace is back
  (`View::raw`, tested).

Compare the two side by side with
`ksee shot OUT.png --url=https://shawnzhong.com/kSEE/#ex=pipe`
against `ksee shot OUT.png --at='#ex=pipe'` (with `CHROME` set). `./ksee.sh test --ui` passes.

## Left to do

- Lanes are 300px columns, so in a 340px lesson column the second lane
  scrolls sideways. The old page did the same, but it could be better
  (narrow lanes wrap badly: see the pipe lesson).
- Old features not brought back:
  - the "level" stepping (◀ ▶ at a depth; ▲ ▼ now choose the frame);
  - a value's history of writes on its row (hovering shows who last wrote
    it).

  Ask before adding them back.

## Notes

- Measured:
  - Dioxus adds about 140 KB gzipped;
  - a step takes 16 ms median, 45 ms max (about 900 chart bars over the
    whole read recording).
- `with_context` needs `Send`, so the Machine lives in a thread-local
  (`MACHINE`).
- A key is only allowed on the first node of an rsx block; use
  `Fragment { key }`.
- `use_resource(use_reactive!(|(p,)| …))` refetches as `p` changes. An
  effect that scrolls must also depend on what was loaded: source.rs
  re-centres once the file loads, or the browser clamps the scroll to the
  previous file's height.
- Memos are stale inside the handler that changed what they depend on:
  compute directly there (`Ui::lines_of`, as `follow` and `select` do).
