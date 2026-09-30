// Values as trees (the Stack's): a row a value, its name, value and type;
// pointers open into what they point to, structs into their fields (a
// click on the name), read
// from memory as it was at the cursor (Machine.memory_at). Values that
// changed since the cursor last moved are highlighted; hover one in memory
// for who last wrote it (a value's changes over time: the Objects', a
// strip on each member the selected call writes).
import { el, taskName } from "./dom.js";


// `st`: a tree's state: { open (paths open), last, now (values shown
// before and now, by path) }; `redraw()`: draw it again (a value opened or
// closed).
export function tree(app, st, redraw) {
  const { m } = app;
  st.open ??= new Set();
  (st.last ??= new Map()), (st.now ??= new Map());
  // (Another moment: the values shown so far are the ones to compare with.)
  const moved = () => ((st.last = st.now), (st.now = new Map()));
  const toggle = (set, p) => (set.has(p) ? set.delete(p) : set.add(p), redraw());
  // The children of value `o` (an Object, see ksee_core::view), if it has any.
  function kids(o) {
    if (o.children) return o.children;
    if (o.expand != null) return m.object_by_id(o.name, o.expand, o.addr, 1)?.children ?? [];
    if (o.target) return m.object_by_id(o.name, o.target.ty, o.target.addr, 1)?.children ?? [];
    return null;
  }

  // Rows for `list` under `path`, `depth` deep, into `out`; `pre`: their C
  // expressions' start ("file->", "file->f_path.").
  function rows(list, path, depth, out, pre = "") {
    for (const o of list) {
      const p = `${path}.${o.name}`, can = openable(o), open = can && st.open.has(p);
      const value = o.value ?? "{…}";
      const e = o.name.startsWith("[") ? pre.replace(/(\.|->)$/, "") + o.name : pre + o.name;
      st.now.set(p, value);
      const changed = st.last.has(p) && st.last.get(p) !== value;
      // (One with no value yet, or none the recording has: dim.)
      const none = o.none != null;
      out.push(el("tr", { className: [changed ? "changed" : "", none ? "none" : ""].join(" ").trim(), dataset: { p } },
        // (Its name, or ▸: open it, or fold it.)
        el("td", { className: `fname${can ? " opens" : ""}`, style: `padding-left:${10 + depth * 14}px`,
                   onclick: can ? () => toggle(st.open, p) : null },
          el("span", { className: "tw", textContent: can ? (open ? "▾" : "▸") : "" }), o.name),
        // (Hover a value: who last wrote it, and what it was before. Only a
        // value in memory: not an argument, which is in its call's record.)
        o.addr && o.size
          ? el("td", { className: "fval mem", title: value, onmouseenter: (ev) => (ev.target.title = `${value}\n${wroteIt(o)}`) }, value)
          : el("td", { className: "fval", title: value }, value),
        el("td", { className: "ftype" }, o.ty)));
      if (open) rows(kids(o) ?? [], p, depth + 1, out, o.target != null ? `${e}->` : `${e}.`);
    }
  }
  // Who last wrote value `o`, and where from: back through the copies
  // that brought it there (Machine.origin), a line each.
  function wroteIt(o) {
    const way = m.origin(o.addr, Math.min(o.size, 8));
    if (!way.length) return "not written in the recording";
    const [w, ...back] = way, last = way.at(-1);
    return [`written at ${place(w.at)}, by ${taskName(w.task)}; was ${w.old}`,
      ...back.map((x) => `  ← copied from 0x${x.addr.toString(16)}, written at ${place(x.at)}`),
      ...(last.from != null ? [`  ← copied from 0x${last.from.toString(16)}, not written in the recording`] : [])].join("\n");
  }
  return { rows, kids, moved };
}

// Whether value `o` opens: into its fields, or what it points to.
export const openable = (o) => o.children || o.expand != null || o.target != null;

// Where a write was: "ksys_read fs/read_write.c:720".
export const place = (at) => (at ? `${at.inlined ?? at.func} ${at.file}:${at.line}` : "?");
