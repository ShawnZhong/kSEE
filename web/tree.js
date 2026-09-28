// Values as trees (the Stack's): a row a value, its name, value and type;
// pointers open into what they point to, structs into their fields (a
// click on the name), read
// from memory as it was at the cursor (Machine.memory_at). Values that
// changed since the cursor last moved are highlighted; hover one in memory
// for who last wrote it; click it for its writes (Machine.writes), a row
// under it: each the member, from what to what, in which function, a link
// to that moment, the one in effect at the cursor marked; a local's writes
// while its frame lives, another value's in the whole recording (one never
// written: none, as its hover says); click the value again to close.
import { el, taskName } from "./dom.js";

// The most writes of a value shown (the newest).
const WRITES = 256;

// `st`: a tree's state: { open (paths open), history (paths whose writes
// are shown), last, now (values shown before and now, by path) }; `redraw()`: draw it again
// (a value opened or closed).
export function tree(app, st, redraw) {
  const { m } = app;
  st.history ??= new Set();
  const toggle = (set, p) => (set.has(p) ? set.delete(p) : set.add(p), redraw());
  // The children of value `o` (an Object, see ksee_core::view), if it has any.
  function kids(o) {
    if (o.children) return o.children;
    if (o.expand != null) return m.object_by_id(o.name, o.expand, o.addr, 1)?.children ?? [];
    if (o.target != null && o.value !== "NULL") return m.object_by_id(o.name, o.target, Number(o.value), 1)?.children ?? [];
    return null;
  }

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
        // (Its name, or ▸: open it, or fold it.)
        el("td", { className: `fname${can ? " opens" : ""}`, style: `padding-left:${10 + depth * 14}px`,
                   onclick: can ? () => toggle(st.open, p) : null },
          el("span", { className: "tw", textContent: can ? (open ? "▾" : "▸") : "" }), o.name),
        // (Hover a value: who last wrote it, and what it was before; click:
        // its writes. Only a value in memory: not an argument, which is in
        // its call's record, at 0.)
        o.addr && o.size
          ? el("td", { className: "fval mem", title: value, onmouseenter: (ev) => (ev.target.title = `${value}\n${wroteIt(o)}\n(click: its writes)`),
                       onclick: () => (st.history.has(p) || writesOf(o, life).length) && toggle(st.history, p) }, value)
          : el("td", { className: "fval", title: value }, value),
        el("td", { className: "ftype", textContent: o.ty })));
      if (st.history.has(p)) out.push(history(o, p, life, depth));
      // (A pointer's target is another object: its own lifetime.)
      if (open) rows(kids(o) ?? [], p, depth + 1, out, o.target != null ? null : life, o.target != null ? `${e}->` : `${e}.`);
    }
  }
  // Value `o`'s writes (Machine.writes): a local's while its frame lives
  // (`life`), another's in the whole recording.
  const writesOf = (o, life) => m.writes(o.addr, o.id, life?.from, life?.to, WRITES);
  // Its writes, a row; the write in effect at the cursor marked.
  function history(o, p, life, depth) {
    const cur = app.time.cursor() ?? Infinity, writes = writesOf(o, life);
    const now = writes.findLastIndex((x) => x.i <= cur);
    const items = writes.map((x, j) => el("li", { className: j === now ? "on" : "" },
      el("a", { textContent: `${x.member ? `${x.member}: ` : ""}${x.before} → ${x.after}`, title: "go to this write", onclick: () => app.time.go(x.i) }),
      el("span", { className: "dim", textContent: ` in ${x.at?.func ?? "?"}` })));
    return el("tr", { className: "history" }, el("td", { colSpan: 3, style: `padding-left:${24 + depth * 14}px` },
      el("div", { className: "dim" }, `${writes.length}${writes.length >= WRITES ? "+" : ""} write${writes.length === 1 ? "" : "s"} ${life?.what ?? "in the recording"}`),
      items.length ? el("ol", {}, ...items) : null));
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
  return { rows, kids };
}

// Whether value `o` opens: into its fields, or what it points to.
export const openable = (o) => o.children || o.expand != null || (o.target != null && o.value !== "NULL");

// Where a write was: "ksys_read fs/read_write.c:720".
export const place = (at) => (at ? `${at.inlined ?? at.func} ${at.file}:${at.line}` : "?");
