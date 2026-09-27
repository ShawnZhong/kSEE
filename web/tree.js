// Values as trees (the Stack's and the Objects'): a row a value, its
// name, value and type; pointers open into what they point to, structs
// into their fields, read from memory as it was at the cursor
// (Machine.memory_at). Values that changed since the cursor last moved are
// highlighted; hover one in memory for who last wrote it, click it to
// pin it in the Objects (see objects.js).
import { el } from "./dom.js";

// `st`: a tree's state: { open (paths open), last, now (values shown
// before and now, by path) }; `redraw()`: draw it again (a value opened or
// closed); `watch(o, life)`: a value clicked.
export function tree(app, st, redraw, watch) {
  const { m } = app;
  // The children of value `o` (an Object, see ksee_core::view), if it has any.
  function kids(o) {
    if (o.children) return o.children;
    if (o.expand != null) return m.object_by_id(o.name, o.expand, o.addr, 1)?.children ?? [];
    if (o.target != null && o.value !== "NULL") return m.object_by_id(o.name, o.target, Number(o.value), 1)?.children ?? [];
    return null;
  }
  const openable = (o) => o.children || o.expand != null || (o.target != null && o.value !== "NULL");

  // Rows for `list` under `path`, `depth` deep, into `out`; `pre`: their C
  // expressions' start ("file->", "file->f_path."); `life`: their
  // object's lifetime, for its history ({ from, to, what }: a local's
  // frame, from its call to its return; null: the whole recording).
  function rows(list, path, depth, out, life = null, pre = "") {
    for (const o of list) {
      const p = `${path}.${o.name}`, can = openable(o), open = can && st.open.has(p);
      const value = o.value ?? "{…}";
      const e = o.name.startsWith("[") ? pre.replace(/(\.|->)$/, "") + o.name : pre + o.name;
      st.now.set(p, value);
      const changed = st.last.has(p) && st.last.get(p) !== value;
      out.push(el("tr", { className: changed ? "changed" : "" },
        el("td", { className: "fname", style: `padding-left:${10 + depth * 14}px` },
          el(can ? "a" : "span", { className: "tw", textContent: can ? (open ? "▾" : "▸") : "", onclick: () => {
            open ? st.open.delete(p) : st.open.add(p);
            redraw();
          } }), o.name),
        // (Hover a value: who last wrote it, and what it was before; click:
        // watch it. Only a value in memory: not an argument, which is in its
        // call's record, at 0.)
        o.addr && o.size
          ? el("td", { className: "fval mem", title: value, onmouseenter: (ev) => (ev.target.title = `${value}\n${wroteIt(o)}\n(click: watch it)`),
                       onclick: () => watch({ ...o, name: e }, life) }, value)
          : el("td", { className: "fval", title: value }, value),
        el("td", { className: "ftype", textContent: o.ty })));
      // (A pointer's target is another object: its own lifetime.)
      if (open) rows(kids(o) ?? [], p, depth + 1, out, o.target != null ? null : life, o.target != null ? `${e}->` : `${e}.`);
    }
  }
  // The newest write to `o` at the cursor (Machine.last_write): where, by
  // whom, and the value it overwrote.
  function wroteIt(o) {
    const w = m.last_write(o.addr, Math.min(o.size, 8));
    if (!w) return "not written in the recording";
    return `written at ${place(w.at)}, by ${w.task.comm ?? "?"}(${w.task.pid ?? "?"}); was ${w.old}`;
  }
  return { rows, kids };
}

// Where a write was: "ksys_read fs/read_write.c:720".
export const place = (at) => (at ? `${at.inlined ?? at.func} ${at.file}:${at.line}` : "?");
