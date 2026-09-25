// The Stack tab: the call stack at the cursor, innermost first, each call
// with its arguments and its locals in memory as a tree (like a debugger's
// variables), rebuilt from the trace (Machine.call_stack: a call's
// arguments are in its record, its locals' addresses in another);
// then the watches: the task at the cursor, and expressions (a global, or
// a call's argument, and fields: "file->f_pos"; see resolve). Pointers open into
// what they point to, structs into their fields, read from memory as it was
// at the cursor (Machine.memory_at); what is open stays open while
// stepping; values that changed since the cursor last moved are
// highlighted; ✎: which code last wrote a value. Click a call for its
// source.
import { $, el } from "./dom.js";

export function callstack(app) {
  const { m } = app;
  // Open paths, and the values shown before (to highlight changes).
  const s = { open: new Set(), last: new Map(), now: new Map(), frames: [], task: null, watches: [] };

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
        el("td", { className: "fval", title: value }, value, o.value != null && o.addr ? app.writer(o) : null),
        el("td", { className: "ftype", textContent: o.ty })));
      if (open) rows(kids(o) ?? [], p, depth + 1, out);
    }
  }
  const head = (text, where, onclick, cls = "") => el("tr", { className: "frame " + cls },
    el("td", { colSpan: 3 }, el("a", { className: "mono", textContent: text, onclick }),
      el("span", { className: "dim", textContent: where ? `  ${where}` : "" })));

  function draw() {
    s.now = new Map();
    const n = s.frames.length, body = [];
    s.frames.forEach((f, k) => {
      const h = head(f.func, f.where, () => app.showFunction(f.func.replace(/ \(inlined\)$/, ""), undefined,
        k === 0 ? f.line : undefined, k === 0 ? f.file : undefined), k === 0 ? "on" : "");
      // (Hover: the call, outlined in the timeline.)
      if (f.first != null) {
        h.onmouseenter = () => app.time.hilite(f.first);
        h.onmouseleave = () => app.time.hilite(null);
      }
      body.push(h);
      if (f.args) rows(f.args, `${n - k}/${f.func}`, 1, body);
      if (f.locals?.length) {
        body.push(el("tr", { className: "sub" }, el("td", { colSpan: 3, textContent: "locals" })));
        rows(f.locals, `${n - k}/${f.func}/locals`, 1, body);
      }
    });
    // The watches: the task at the cursor, then the globals asked for.
    const watches = [];
    if (s.task) watches.push({ title: `current: ${s.task.comm ?? "?"}(${s.task.pid ?? "?"})`, key: "task",
                               value: () => m.object("task_struct", s.task.addr, 1) });
    for (const g of s.watches) watches.push({ title: g, key: `watch/${g}`, value: () => resolve(g), remove: g });
    if (watches.length) body.push(el("tr", { className: "sep" }, el("td", { colSpan: 3, textContent: "watch" })));
    for (const w of watches) {
      const o = w.value();
      const h = head(w.title, "", () => {
        s.open.has(w.key) ? s.open.delete(w.key) : s.open.add(w.key);
        draw();
      }, "watch");
      if (w.remove) h.firstChild.append(el("a", { className: "who", textContent: "  ✕", title: "stop watching", onclick: () => {
        s.watches = s.watches.filter((x) => x !== w.remove);
        draw();
      } }));
      body.push(h);
      if (!o) body.push(el("tr", {}, el("td", { colSpan: 3, className: "dim", style: "padding-left:24px", textContent: "not here (no such name at the cursor)" })));
      else if (s.open.has(w.key)) rows(o.children ?? [o], w.key, 1, body);
    }
    $("stack").replaceChildren(el("table", { className: "objs stack" }, el("tbody", {}, ...body)));
  }

  // The stack at record `i` (null: the last; undefined: none), `step` the
  // line shown there (Machine.line_steps; its inlined function, if any, is a
  // frame of its own); `task`: the task there.
  function show(i, step, task) {
    s.last = s.now;
    s.task = task ?? null;
    const frames = i === undefined ? [] : m.call_stack(i ?? undefined);
    s.frames = frames.map((f, k) => ({ ...f, where: k === 0 && step ? `${step.file}:${step.line}` : f.file,
      line: k === 0 ? step?.line : undefined, file: k === 0 ? step?.file ?? f.file : f.file }));
    if (step?.inlined) s.frames.unshift({ func: `${step.inlined} (inlined)`, where: `${step.file}:${step.line}`, line: step.line, file: step.file });
    // (The stack comes from the recording: nothing recorded, no stack.)
    $("stack-note").textContent = frames.length ? "" : m.recording() || m.records()
      ? "The call stack at the cursor, with arguments: pick a moment in the recording."
      : "Not recording: the call stack comes from the recording. Run a tour, or ● Record.";
    draw();
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

  // Watch expression `expr` (opened); `only` as for resolve, to check it.
  function watch(expr, only) {
    if (!resolve(expr, only)) return false;
    if (!s.watches.includes(expr)) s.watches.push(expr);
    s.open.add(`watch/${expr}`);
    draw();
    return true;
  }
  $("globals").append(...m.globals().map((g) => el("option", { value: g })));
  $("watchf").onsubmit = (e) => {
    e.preventDefault();
    if (watch($("watch").value.trim())) $("watch").value = "";
  };
  return { show, watch, resolve, kids, redraw: draw };
}
