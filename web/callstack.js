// The Stack tab: the call stack at the cursor, innermost first, each call
// with its arguments and its locals in memory as a tree (like a debugger's
// variables), rebuilt from the trace (Machine.call_stack: a call's
// arguments are in its record, its locals' addresses in another); at the
// bottom, the task running then (current: its key fields, open, and all
// of them on asking). Pointers open into
// what they point to, structs into their fields, read from memory as it was
// at the cursor (Machine.memory_at); what is open stays open while
// stepping; values that changed since the cursor last moved are
// highlighted; hover a value for who last wrote it, click it for its
// history (every write to it, each a link to that moment). Click a call to select
// it: the source shows where it is (for an outer call, the line calling
// the next one in).
import { $, el } from "./dom.js";

export function callstack(app) {
  const { m } = app;
  // Open paths, and the values shown before (to highlight changes).
  // (The current task open to begin with.)
  const s = { open: new Set(["task"]), last: new Map(), now: new Map(), frames: [], task: null, sel: null };
  // The current task's fields shown first.
  const KEY = ["pid", "tgid", "comm", "__state", "flags", "prio", "mm", "files", "fs", "real_parent", "group_leader", "signal"];

  // The children of value `o` (an Object, see ksee_core::view), if it has any.
  function kids(o) {
    if (o.children) return o.children;
    if (o.expand != null) return m.object_by_id(o.name, o.expand, o.addr, 1)?.children ?? [];
    if (o.target != null && o.value !== "NULL") return m.object_by_id(o.name, o.target, Number(o.value), 1)?.children ?? [];
    return null;
  }
  const openable = (o) => o.children || o.expand != null || (o.target != null && o.value !== "NULL");

  function rows(list, path, depth, out) {
    for (const o of list) {
      const p = `${path}.${o.name}`, can = openable(o), open = can && s.open.has(p);
      const value = o.value ?? "{…}";
      s.now.set(p, value);
      const changed = s.last.has(p) && s.last.get(p) !== value;
      out.push(el("tr", { className: changed ? "changed" : "" },
        el("td", { className: "fname", style: `padding-left:${10 + depth * 14}px` },
          el(can ? "a" : "span", { className: "tw", textContent: can ? (open ? "▾" : "▸") : "", onclick: () => {
            open ? s.open.delete(p) : s.open.add(p);
            draw();
          } }), o.name),
        // (Hover a value: who last wrote it, and what it was before; click: its history.)
        // (Only a value in memory has one, a scalar: not an argument, which is in its call's record.)
        o.addr && o.size && o.value != null
          ? el("td", { className: "fval mem", title: value, onmouseenter: (e) => (e.target.title = `${value}\n${wroteIt(o)}\n(click: its history)`),
                       onclick: () => ((s.history = s.history === p ? null : p), draw()) }, value)
          : el("td", { className: "fval", title: value }, value),
        el("td", { className: "ftype", textContent: o.ty })));
      if (s.history === p) out.push(history(o, depth));
      if (open) rows(kids(o) ?? [], p, depth + 1, out);
    }
  }
  // Value `o`'s history (Machine.writes): its writes, the newest 8, each a
  // link to that moment; the one in effect at the cursor marked.
  function history(o, depth) {
    const all = m.writes(o.addr, Math.min(o.size, 8));
    const cur = app.time.cursor() ?? Infinity, now = all.findLastIndex((w) => w.i <= cur);
    const items = all.slice(-8).map((w) => {
      const at = w.at ? `${w.at.inlined ?? w.at.func} ${w.at.file}:${w.at.line}` : "?";
      return el("li", { className: all.indexOf(w) === now ? "on" : "" },
        el("a", { textContent: `${w.before} → ${w.after}`, onclick: () => app.time.go(w.i) }),
        el("span", { className: "dim", textContent: ` ${at} · ${w.task.comm ?? "?"}(${w.task.pid ?? "?"})` }));
    });
    return el("tr", { className: "history" }, el("td", { colSpan: 3, style: `padding-left:${24 + depth * 14}px` },
      items.length ? el("ol", {}, ...items) : el("span", { className: "dim", textContent: "not written in the recording" }),
      all.length > 8 ? el("div", { className: "dim", textContent: `(${all.length - 8} earlier)` }) : null));
  }
  // The newest write to `o` at the cursor (Machine.last_write): where, by
  // whom, and the value it overwrote.
  function wroteIt(o) {
    if (!o.addr || !o.size) return "";
    const w = m.last_write(o.addr, o.size);
    if (!w) return "not written in the recording";
    const at = w.at ? `${w.at.inlined ?? w.at.func} ${w.at.file}:${w.at.line}` : "?";
    return `written at ${at}, by ${w.task.comm ?? "?"}(${w.task.pid ?? "?"}); was ${w.old}`;
  }
  const head = (text, where, onclick, cls = "") => el("tr", { className: "frame " + cls },
    el("td", { colSpan: 3 }, el("a", { className: "mono", textContent: text, onclick }),
      el("span", { className: "dim", textContent: where ? `  ${where}` : "" })));

  function draw() {
    s.now = new Map();
    const n = s.frames.length, body = [];
    s.frames.forEach((f, k) => {
      // (Click: select the call; the source shows where it is.)
      const on = s.sel == null ? k === 0 : f === s.sel;
      // (The syscall entry: named for the syscall, its arguments the syscall's.)
      const h = head(f.syscall ? `${f.syscall}()` : f.func, f.line ? `${f.file}:${f.line}` : f.file, () => app.selectFrame(f), on ? "on" : "");
      // (Hover: the call outlined in the timeline, and a preview of where it is.)
      h.onmouseenter = () => {
        app.time.hilite(f.first);
        app.preview(() => ({ func: f.inlined ? owner(f)?.func ?? f.func : f.func, file: f.file, line: f.line }));
      };
      h.onmouseleave = () => {
        app.time.hilite(null);
        app.preview(null);
      };
      body.push(h);
      if (f.args) rows(f.args, `${n - k}/${f.func}`, 1, body);
      if (f.locals?.length) {
        body.push(el("tr", { className: "sub" }, el("td", { colSpan: 3, textContent: "locals" })));
        rows(f.locals, `${n - k}/${f.func}/locals`, 1, body);
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
    $("stack").replaceChildren(el("table", { className: "objs stack" }, el("tbody", {}, ...body)));
  }

  // The stack at record `i` (null: the last; undefined: none), `step` the
  // line shown there ({ nth }: which of the lines record `i` runs); `task`:
  // the task there.
  function show(i, step, task) {
    kept = null;
    $("stack").classList.remove("preview");
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
    draw();
  }

  // Preview the stack at record `i` (as it runs there; memory as it was:
  // see app.preview), the cursor's kept; unpreview: back to it.
  let kept = null;
  function preview(i) {
    kept ??= { frames: s.frames, task: s.task, sel: s.sel };
    s.frames = m.call_stack(i);
    s.task = m.at(i)?.task ?? null;
    s.sel = null;
    // (Values unlike the cursor's marked.)
    draw();
    $("stack").classList.add("preview");
  }
  function unpreview() {
    if (!kept) return;
    Object.assign(s, kept);
    kept = null;
    draw();
    $("stack").classList.remove("preview");
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

  return { show, preview, unpreview, select, level, owner, resolve, kids };
}
