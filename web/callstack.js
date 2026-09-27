// The Stack: the call stack at the cursor, innermost first, each call
// with its arguments and its locals in memory as a tree (like a debugger's
// variables), rebuilt from the trace (Machine.call_stack: a call's
// arguments are in its record, its locals' addresses in another); at the
// bottom, the task running then (current: its key fields, open, and all
// of them on asking). Pointers open into
// what they point to, structs into their fields, read from memory as it was
// at the cursor (Machine.memory_at: see tree.js); what is open stays
// open while stepping; click a value for its writes (tree.js). Click a call to
// select it: the source shows where it is (for an outer call, the line
// calling the next one in).
import { $, el } from "./dom.js";
import { tree, place } from "./tree.js";

export function callstack(app) {
  const { m } = app;
  // Open paths, and the values shown before (to highlight changes).
  // (The current task open to begin with.)
  const s = { open: new Set(["task"]), last: new Map(), now: new Map(), frames: [], task: null, sel: null };
  // The current task's fields shown first.
  const KEY = ["pid", "tgid", "comm", "__state", "flags", "prio", "mm", "files", "fs", "real_parent", "group_leader", "signal"];

  const { rows, kids } = tree(app, s, draw);
  // (The tabs: the pane chosen.)
  for (const a of document.querySelectorAll("h2.tabs a")) a.onclick = () => ($("right").dataset.tab = a.dataset.tab);
  const head = (text, where, onclick, cls = "") => el("tr", { className: "frame " + cls },
    el("td", { colSpan: 3 }, el("a", { className: "mono", textContent: text, onclick }),
      el("span", { className: "dim", textContent: where ? `  ${where}` : "" })));

  function draw() {
    s.now = new Map();
    const n = s.frames.length, body = [];
    s.frames.forEach((f, k) => {
      // (Click: select the call; the source shows where it is.)
      const on = s.sel == null ? k === 0 : f === s.sel;
      // (The syscall entry: which syscall, its arguments the syscall's.)
      const h = head(f.func, `${f.line ? `${f.file}:${f.line}` : f.file}${f.inlined ? " · inlined" : ""}${f.syscall ? ` · ${f.syscall}` : ""}`, () => app.selectFrame(f), on ? "on" : "");
      // (Hover: the call outlined in the timeline.)
      h.onmouseenter = () => app.time.hilite(f.first);
      h.onmouseleave = () => app.time.hilite(null);
      body.push(h);
      if (f.args) rows(f.args, `${n - k}/${f.func}`, 1, body);
      if (f.locals?.length) {
        body.push(el("tr", { className: "sub" }, el("td", { colSpan: 3, textContent: "locals" })));
        rows(f.locals, `${n - k}/${f.func}/locals`, 1, body, { from: f.first, to: m.call_end(f.first) ?? undefined, what: `while ${f.func}'s frame lives` });
      }
    });
    // At the bottom: the task running then (its task_struct, as it was).
    if (s.task && s.frames.length) {
      const toggle = (p) => () => {
        s.open.has(p) ? s.open.delete(p) : s.open.add(p);
        draw();
      };
      body.push(head(`current task: ${s.task.comm ?? "?"}(${s.task.pid ?? "?"})`, "current: the task running", toggle("task"), "task"));
      if (s.open.has("task")) {
        const all = m.object("task_struct", s.task.addr, 1)?.children ?? [], full = s.open.has("task/all");
        rows(full ? all : KEY.map((k) => all.find((o) => o.name === k)).filter(Boolean), "task", 1, body);
        body.push(el("tr", { className: "sub" }, el("td", { colSpan: 3 },
          el("a", { textContent: full ? "▾ the key fields only" : `▸ all ${all.length} fields`, onclick: toggle("task/all") }))));
      }
    }
    $("stack").replaceChildren(el("div", { className: "spacer" }), el("table", { className: "objs stack" }, el("tbody", {}, ...body)));
    focus();
  }
  // Above a selected call, room for the calls that can still appear there
  // (the deepest stack over its bar in the timeline, less those shown),
  // each about a frame's height now: as the pointer moves along the bar
  // and its callees come and go, the selected row stays put. A row that
  // would be low in the pane is scrolled to a third of the way down.
  function focus() {
    const pane = $("stack"), on = pane.querySelector("tr.frame.on"), table = pane.querySelector("table"), spacer = pane.querySelector(".spacer");
    const rows = [...pane.querySelectorAll("tr.frame")], k = on ? rows.indexOf(on) : 0;
    if (!on || !s.sel) return (spacer.style.height = "0px");
    const need = Math.max(0, app.time.deepest(s.sel.first) - k), each = table.offsetHeight / Math.max(1, rows.length);
    spacer.style.height = `${Math.round(need * each)}px`;
    spacer.textContent = need ? "what it calls shows here" : "";
    const top = on.getBoundingClientRect().top - pane.getBoundingClientRect().top + pane.scrollTop;
    if (top > pane.clientHeight * 0.66) pane.scrollTop = top - pane.clientHeight / 3;
  }

  // The stack at record `i` (null: the last; undefined: none), `step` the
  // line shown there ({ nth }: which of the lines record `i` runs); `task`:
  // the task there.
  // The frames shown.
  function show(i, step, task) {
    s.last = s.now;
    s.task = task ?? null;
    app.trace?.mark();
    // (At a line step: the stack as that line runs; see Machine.call_stack.)
    const frames = i === undefined ? [] : m.call_stack(i ?? undefined, step?.nth);
    s.frames = frames;
    s.sel = null;
    // (The stack comes from the recording: nothing recorded, no stack.)
    $("stack-note").textContent = frames.length ? "" : m.records()
      ? "The call stack at the cursor, with arguments: pick a moment in the recording."
      : "Nothing recorded yet: the call stack comes from the recording. Make a call.";
    draw();
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
    draw();
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

  return { show, select, level, owner, neighbour, frames: () => s.frames, resolve, kids };
}
