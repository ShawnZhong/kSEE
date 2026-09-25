// The Stack tab: the call stack at the cursor, innermost first, each call
// with its arguments as a tree (like a debugger's variables). Paused at
// now, it is the kernel's own (the plugin's frames on the kernel stack):
// pointers open into what they point to, structs into their fields, read
// from memory as it is, and what is open stays open while stepping; values
// that changed since the last stop are highlighted. In the history, it is
// rebuilt from the trace (names only: memory is not recorded). Click a
// call for its source.
import { $, el } from "./dom.js";

export function callstack(app) {
  const { m } = app;
  // Open paths ("depth-from-outermost/func/arg.field..."), and the values
  // shown at the last stop (to highlight changes).
  const s = { open: new Set(), last: new Map(), now: new Map(), frames: [] };

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
      const value = o.value ?? (o.children ? "{…}" : "{…}");
      s.now.set(p, value);
      const changed = s.last.has(p) && s.last.get(p) !== value;
      out.push(el("tr", { className: changed ? "changed" : "" },
        el("td", { className: "fname", style: `padding-left:${10 + depth * 14}px` },
          el(can ? "a" : "span", { className: "tw", textContent: can ? (open ? "▾" : "▸") : "", onclick: () => {
            open ? s.open.delete(p) : s.open.add(p);
            draw();
          } }), o.name),
        el("td", { className: "fval", title: value }, value, o.value != null ? app.writer(o) : null,
          o.target != null && o.value !== "NULL" ? el("a", { className: "follow", textContent: " ↗", title: "open in Objects",
            onclick: () => app.inspect(o.name, o.target, Number(o.value)) }) : null),
        el("td", { className: "ftype", textContent: o.ty })));
      if (open) rows(kids(o) ?? [], p, depth + 1, out);
    }
  }

  function draw() {
    s.now = new Map();
    const n = s.frames.length;
    const body = s.frames.flatMap((f, k) => {
      const key = `${n - k}/${f.func}`;
      const head = el("tr", { className: "frame" + (k === 0 ? " on" : "") },
        el("td", { colSpan: 3 }, el("a", { className: "mono", textContent: f.func, onclick: () =>
          app.showFunction(f.func, undefined, k === 0 ? f.line : undefined, k === 0 ? f.file : undefined) }),
          el("span", { className: "dim", textContent: f.where ? `  ${f.where}` : "" })));
      const out = [head];
      if (f.args) rows(f.args, key, 1, out);
      return out;
    });
    $("stack").replaceChildren(el("table", { className: "objs stack" }, el("tbody", {}, ...body)));
  }

  // At a stop (the kernel's frames, with arguments) or at record `at`
  // (rebuilt from the trace); `step`: the line (Machine.line_steps) shown
  // there, whose inlined function, if any, is a frame of its own.
  function show(at, stopped, step) {
    if (stopped) {
      const here = step ?? stopped;
      s.frames = m.call_stack().map((f, k) => ({ ...f, line: k === 0 ? here.line : undefined,
        file: k === 0 ? here.file : f.file, where: k === 0 ? `${here.file}:${here.line}` : f.file }));
      if (step?.inlined) s.frames.unshift({ func: `${step.inlined} (inlined)`, where: `${step.file}:${step.line}`, line: step.line, file: step.file });
      $("stack-note").textContent = "the kernel's, with arguments (live memory)";
      draw();
      return;
    }
    if (at?.i == null) {
      s.frames = [];
      $("stack").replaceChildren(el("div", { className: "empty", textContent: "Pause the kernel (or run a tour) for its call stack, with arguments; or pick a moment in the history." }));
      $("stack-note").textContent = "";
      return;
    }
    const here = step ?? at;
    s.frames = m.stack_at(at.i).reverse().map((func, k) => ({ func, where: k === 0 ? `${here.file}:${here.line}` : "",
      line: k === 0 ? here.line : undefined, file: k === 0 ? here.file : undefined }));
    if (step?.inlined) s.frames.unshift({ func: `${step.inlined} (inlined)`, where: `${step.file}:${step.line}`, line: step.line, file: step.file });
    $("stack-note").textContent = "from the trace (arguments only while paused)";
    draw();
  }
  // A new stop: the values shown become the ones to compare with.
  function stopped() {
    s.last = s.now;
  }
  return { show, stopped };
}
