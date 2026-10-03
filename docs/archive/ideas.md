# Historical ideas

> This is an archived idea list, not a roadmap or a source of current
> implementation status. It records proposals that were still open when the
> list was archived. Check the current code, issues, and primary documentation
> before treating any item as active work.

The earlier reasoning is in repository history, including the deleted
`docs/info-proposals.md`, `docs/feature-ideas.md`, and
`docs/deletion-menu.md`. Those proposal documents remain deleted; this archive
does not replace them.

## Open at the time of archival

Show more of what the recording already has:

- At a context switch, say who hands over to whom from the switch call's `prev`
  and `next` arguments.
- In Tasks, link a task row to where it last ran (`TaskAt.last`).
- In Objects, name the writer's source line rather than only its function
  (`Run.at`).
- Show a value's first copy-origin hop (`origin`) as a compact history.
- When stepping back across a bulk store whose old bytes were not retained,
  explain that those earlier bytes are shown as they are now.

Candidates that were proposed for removal, subject to user value:

- Editor completions and signature hints (`program/complete.rs`, `editor.rs`).
- Persisted pane sizes or pane drag handles.
- The chart function-search box.
- `ksee deploy`, if publishing moves elsewhere.
- The `umask-inside` lesson, which overlaps the `umask` lesson.

## Obsolete item

The source now marks sealed functions and explains that their internal turns
are not recorded. That proposal was implemented before archival and has been
removed from the open list.
