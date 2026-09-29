// Stepping through a selection's lines (Machine.line_steps), as ← → do:
// which line is next, from where the cursor is, at a level or at every
// line. Pure: tests/steps.js tries it on made-up lines.

// Whether line `s` is deeper than `level` ({ depth, inl }: calls deep, then
// inlined deep; null: no level, nothing is).
export const deeper = (s, level) => level != null && (s.depth > level.depth || (s.depth === level.depth && s.inl > level.inl));

// The line after (`dir` 1) or before (-1) the one at index `k` of `steps`,
// or -1 if none. `between`: the cursor is not at line `k` but just before
// it (between two lines: after a click in the timeline), so going on it is
// the next. At a level (see deeper): the next that deep or shallower (what
// the calls there make is stepped over; the caller's lines, and a
// sibling's at the level, are not). With none: the next line, but not a
// leaf of inlined code (a helper that calls nothing: its lines pass, the
// line using it stays) nor, after one, the same line again (it goes on).
export function next(steps, k, dir, level, between = false) {
  const here = steps[k], same = (s) => here && s.file === here.file && s.line === here.line && s.depth === here.depth && s.inl === here.inl;
  let passed = false;
  for (let j = between && dir > 0 ? k : k + dir; j >= 0 && j < steps.length; j += dir) {
    const s = steps[j];
    if (level) {
      if (!deeper(s, level)) return j;
    } else if (s.leaf) passed = true;
    else if (!(passed && same(s))) return j;
  }
  return -1;
}
