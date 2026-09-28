// The Stack: the call stack at the cursor, innermost first, a row a call;
// the selected one (the innermost, unless another is: a click on it, ▲ ▼,
// a bar in the timeline) with its arguments and its locals as a tree, as a
// debugger's variables are the selected frame's (every call's, on
// asking); rebuilt from the trace (Machine.call_stack: a call's arguments
// are in its record, its locals in
// memory at their offsets from its frame, the others as the recording has
// them set). At the bottom, the task running then (a link to it in the
// Tasks). Pointers open into what they point to, structs into their
// fields, read from memory as it was at the cursor (Machine.memory_at: see
// tree.js); what is open stays open while stepping; click a value for its
// writes (tree.js). The source shows where the selected call is (for an
// outer call, the line calling the next one in).
import { $, el, taskName } from "./dom.js";
import { tree, place } from "./tree.js";

export function callstack(app) {
  const { m } = app;
  // Open paths, and the values shown before (to highlight changes).
  const s = { open: new Set(), last: new Map(), now: new Map(), frames: [], task: null, sel: null };

  const { rows, kids } = tree(app, s, () => draw());
  const head = (text, where, onclick, cls = "") => el("tr", { className: "frame " + cls },
    el("td", { colSpan: 3 }, el("a", { className: "mono", textContent: text, onclick }),
      el("span", { className: "dim", textContent: where ? `  ${where}` : "" })));

  // Draw the stack; `moved`: for another moment or another call selected
  // (not a value opened, or its writes: those leave the view where it is).
  function draw(moved = false) {
    s.now = new Map();
    const n = s.frames.length, body = [];
    // (Every call's values, or only the selected one's: on asking.)
    if (n > 1) body.push(el("tr", { className: "sub" }, el("td", { colSpan: 3 },
      el("a", { textContent: s.all ? "▾ only the selected call's values" : "▸ every call's values", onclick: () => ((s.all = !s.all), draw()) }))));
    s.frames.forEach((f, k) => {
      // (Click: select the call; the source shows where it is.)
      const on = s.sel == null ? k === 0 : f === s.sel;
      // (The syscall entry: which syscall, its arguments the syscall's.)
      const h = head(f.func, `${f.line ? `${f.file}:${f.line}` : f.file}${f.inlined ? " · inlined" : ""}${f.syscall ? ` · ${f.syscall}` : ""}`, () => app.selectFrame(f), on ? "on" : "");
      // (Hover: the call outlined in the timeline.)
      h.onmouseenter = () => app.time.hilite(f.first);
      h.onmouseleave = () => app.time.hilite(null);
      body.push(h);
      // (Only the selected call's values, as a debugger's variables are
      // the selected frame's: the others a row each, a click to select.)
      if (!on && !s.all) return;
      if (f.args) rows(f.args, `${n - k}/${f.func}`, 1, body);
      // (Then its locals: one list, as the source has them all as names.)
      if (f.locals) rows(f.locals, `${n - k}/${f.func}/locals`, 1, body, { from: f.first, to: m.call_end(f.first) ?? undefined, what: `while ${f.func}'s frame lives` });
    });
    // At the bottom: the task running then, a link to it in the Tasks.
    if (s.task && s.frames.length)
      body.push(head(`current task: ${taskName(s.task)}`, "current: the task running; its task_struct in the Tasks", () => app.tasks.open(s.task.addr), "task"));
    // (At the bottom of the pane, as the timeline's stacks: the calls that
    // come and go as the cursor moves are on top, the rows under them stay
    // put: moved, scrolled as far from the bottom as it was, the selected
    // call in view; else as it was.)
    const pane = $("stack"), top = pane.scrollTop, fromBottom = pane.scrollHeight - top;
    pane.replaceChildren(el("table", { className: "objs stack" }, el("tbody", {}, ...body)));
    pane.scrollTop = moved ? pane.scrollHeight - fromBottom : top;
    const on = pane.querySelector("tr.frame.on");
    if (on && moved) scrollTo(pane, on);
  }
  // (Row `on` in view, if it is not.)
  function scrollTo(pane, on) {
    const top = on.getBoundingClientRect().top - pane.getBoundingClientRect().top;
    if (top < 0 || top + on.offsetHeight > pane.clientHeight) pane.scrollTop += top - pane.clientHeight / 3;
  }

  // The stack at record `i` (null: the last; undefined: none), `step` the
  // line shown there ({ nth }: which of the lines record `i` runs); `task`:
  // the task there.
  // The frames shown.
  function show(i, step, task) {
    s.last = s.now;
    s.task = task ?? null;
    // (At a line step: the stack as that line runs; see Machine.call_stack.)
    const frames = i === undefined ? [] : m.call_stack(i ?? undefined, step?.nth);
    s.frames = frames;
    s.sel = null;
    // (The stack comes from the recording: nothing recorded, no stack.)
    $("stack-note").textContent = frames.length ? "" : m.records()
      ? "The call stack at the cursor, with arguments: pick a moment in the recording."
      : "Nothing recorded yet: the call stack comes from the recording. Make a call.";
    draw(true);
    return frames;
  }

  // The value of expression `expr` at the cursor: a name, then fields
  // (".f" or "->f"). The name is an argument of a call on the stack (only of
  // function `only`, if given: the code the name is in), else a global.
  function resolve(expr, only) {
    const path = expr.split(/\s*(?:->|\.)\s*/).filter(Boolean);
    if (!path.length) return null;
    const frames = only === undefined ? s.frames : s.frames.filter((f) => f.func === only);
    let o = null;
    for (const f of frames) if ((o = f.args?.find((x) => x.name === path[0]) ?? f.locals?.find((x) => x.name === path[0]))) break;
    o ??= m.global(path[0], 1);
    for (const field of path.slice(1)) {
      if (!o) return null;
      o = kids(o)?.find((c) => c.name === field) ?? null;
    }
    return o ? { ...o, name: expr } : null;
  }

  // Select the call on the stack whose first record is `first` (null: the
  // innermost; `inlined`: code inlined into it, from that function): its
  // frame, or null if it is not on the stack.
  function select(first, inlined) {
    const f = first == null ? s.frames[0]
      : s.frames.find((f) => f.first === first && (inlined ? f.inlined && f.func === inlined : !f.inlined));
    s.sel = f ?? null;
    draw(true);
    return f ?? null;
  }
  // The selected call's level, as line steps have it (calls deep, inlined
  // deep: see Machine.line_steps), or null if none is selected.
  function level() {
    const k = s.sel ? s.frames.indexOf(s.sel) : -1;
    if (k < 0) return null;
    let inl = 0;
    while (s.frames[k + inl]?.inlined) inl++;
    return { depth: s.sel.depth, inl };
  }
  // The call inlined frame `f` is code of.
  const owner = (f) => s.frames.find((x) => x.first === f.first && !x.inlined) ?? null;
  // The frame `dir` away from the selected one (the innermost if none):
  // 1 its caller (the row below: the innermost is on top), -1 its callee;
  // null at the ends.
  const neighbour = (dir) => s.frames[(s.sel ? s.frames.indexOf(s.sel) : 0) + dir] ?? null;

  // The pointer is over the timeline (on), or not.
  const peeking = (on) => (s.peeking = on);

  return { show, select, peeking, level, owner, neighbour, frames: () => s.frames, resolve, kids };
}
