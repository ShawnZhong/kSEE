// Stepping (web/steps.js) on made-up lines: which line ← → go to.
//   deno test tests/steps.js
import { next } from "../web/steps.js";

const eq = (a, b) => {
  if (JSON.stringify(a) !== JSON.stringify(b)) throw new Error(`${JSON.stringify(a)} ≠ ${JSON.stringify(b)}`);
};
// A line: `func` at `line`, `depth` calls deep (inlined `inl` deep).
const L = (func, line, depth, inl = 0, leaf = false) => ({ func, file: "f.c", line, depth, inl, leaf });
// Every stop from `k` going `dir`, at `level`.
const walk = (st, k, dir, level) => {
  const out = [];
  for (let j = next(st, k, dir, level); j >= 0; j = next(st, j, dir, level)) out.push(`${st[j].func}:${st[j].line}`);
  return out;
};

// f (depth 1) calls g (2), which calls h (3); back in f, it calls g2 (2), g's sibling.
const st = [L("f", 1, 1), L("g", 10, 2), L("h", 20, 3), L("h", 21, 3), L("g", 11, 2), L("f", 2, 1), L("g2", 30, 2), L("g2", 31, 2), L("f", 3, 1)];

Deno.test("every line: into every call", () => {
  eq(walk(st, 0, 1, null), ["g:10", "h:20", "h:21", "g:11", "f:2", "g2:30", "g2:31", "f:3"]);
  eq(walk(st, 8, -1, null), ["g2:31", "g2:30", "f:2", "g:11", "h:21", "h:20", "g:10", "f:1"]);
});

Deno.test("at a level: deeper calls stepped over, the caller's lines and the siblings' not", () => {
  // (At g's level, 2: h's lines skipped; f's line between g and g2, and g2's, shown.)
  eq(walk(st, 1, 1, { depth: 2, inl: 0 }), ["g:11", "f:2", "g2:30", "g2:31", "f:3"]);
  eq(walk(st, 7, -1, { depth: 2, inl: 0 }), ["g2:30", "f:2", "g:11", "g:10", "f:1"]);
  // (At f's, 1: only f's lines.)
  eq(walk(st, 0, 1, { depth: 1, inl: 0 }), ["f:2", "f:3"]);
});

Deno.test("inlined code is a level too", () => {
  // f (1) runs line 1, then helper inlined into it (1, inlined 1), then line 2.
  const s = [L("f", 1, 1), L("helper", 5, 1, 1), L("f", 2, 1)];
  eq(walk(s, 0, 1, { depth: 1, inl: 0 }), ["f:2"]);
  eq(walk(s, 0, 1, { depth: 1, inl: 1 }), ["helper:5", "f:2"]);
});

Deno.test("every line: an inlined leaf passes, the line using it stays", () => {
  // f's line 1 uses a helper (a leaf: it calls nothing), then goes on on line 1, then line 2.
  const s = [L("f", 1, 1), L("helper", 5, 1, 1, true), L("f", 1, 1), L("f", 2, 1)];
  eq(walk(s, 0, 1, null), ["f:2"]);
});

Deno.test("between two lines (after a click in the timeline): → goes to the one after the cursor, ← to the one before", () => {
  // (The cursor just before st[4], g:11.)
  eq(`${st[next(st, 4, 1, null, true)].func}:${st[next(st, 4, 1, null, true)].line}`, "g:11");
  eq(`${st[next(st, 4, -1, null, true)].func}:${st[next(st, 4, -1, null, true)].line}`, "h:21");
  // (At a level, the one after it that is not deeper.)
  eq(next(st, 2, 1, { depth: 2, inl: 0 }, true), 4);
});

Deno.test("none further: -1 (the page goes on into the next syscall)", () => {
  eq(next(st, 8, 1, null), -1);
  eq(next(st, 0, -1, { depth: 2, inl: 0 }), -1);
});
