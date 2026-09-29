// Stepping through a selection's lines (Machine.line_steps), as ← → do:
// which line is next, from where the cursor is, at a level or at all of
// them. Pure: tests/steps.js tries it on made-up lines.

// Whether line `s` is deeper than `level` ({ depth, inl }: calls deep, then
// inlined deep, as the Stack's rows are; null: all levels, nothing is).
export const deeper = (s, level) => level != null && (s.depth > level.depth || (s.depth === level.depth && s.inl > level.inl));

// The line after (`dir` 1) or before (-1) the one at index `k` of `steps`
// that is not deeper than `level` (what the calls there make is stepped
// over; the caller's lines, and a sibling's at the level, are not), or -1
// if none. `between`: the cursor is not at line `k` but just before it
// (between two lines: after a click in the timeline), so going on, it is
// the next.
export function next(steps, k, dir, level, between = false) {
  for (let j = between && dir > 0 ? k : k + dir; j >= 0 && j < steps.length; j += dir) {
    if (!deeper(steps[j], level)) return j;
  }
  return -1;
}
