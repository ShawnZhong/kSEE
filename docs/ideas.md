# Ideas not built (yet)

What was proposed and is still open, each a line; the reasoning is in git
history (docs/info-proposals.md, docs/feature-ideas.md,
docs/deletion-menu.md, removed when this replaced them).

Show more of what the recording has:
- At a context switch, say who hands over to whom: `View::switch_at`
  (built and tested, not shown).
- A sealed function (its body not followed): say so in the source.
- A task's row in Tasks: a link to where it last ran (`TaskAt.last`).
- Objects: name the writer's line, not just its function (`Run.at`).
- A value's history in one line: where it was copied from (`origin`,
  the first hop).
- Stepping back over a bulk store (old bytes not kept): say the earlier
  bytes are shown as they are now.

Candidates to remove (the user decides):
- The editor's completions and signature hints (program/complete.rs,
  editor.rs): typing help for "Your own".
- Kept pane sizes (layout.rs storage) or the panes' drag handles.
- The chart's find box.
- `ksee deploy` (if publishing moves out).
- The umask-inside lesson (overlaps umask).
