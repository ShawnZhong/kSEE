// Values as trees (the Stack's): a row a value, its name, value and type;
// pointers open into what they point to, structs into their fields (a
// click on the name), read
// from memory as it was at the cursor (Machine.memory_at). Values that
// changed since the cursor last moved are highlighted; hover one in memory
// for who last wrote it; its row's "writes" for its writes (Machine.writes), a row
// under it: each the member, from what to what, in which function, a link
// to that moment, the one in effect at the cursor marked; a local's writes
// while its frame lives, another value's in the whole recording; "writes"
// again to close.
import { el, taskName } from "./dom.js";

// The most writes of a value shown (the newest).
const WRITES = 256;

// `st`: a tree's state: { open (paths open), history (paths whose writes
// are shown), last, now (values shown before and now, by path) }; `redraw()`: draw it again
// (a value opened or closed).
export function tree(app, st, redraw) {
  const { m } = app;
  st.open ??= new Set();
  st.history ??= new Set();
  (st.last ??= new Map()), (st.now ??= new Map());
  // (Another moment: the values shown so far are the ones to compare with.)
  const moved = () => ((st.last = st.now), (st.now = new Map()));
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
      // (One with no value yet, or none the recording has: dim.)
      const none = /^\((uninitialized|optimized out)/.test(value);
      out.push(el("tr", { className: [changed ? "changed" : "", none ? "none" : ""].join(" ").trim() },
        // (Its name, or ▸: open it, or fold it.)
        el("td", { className: `fname${can ? " opens" : ""}`, style: `padding-left:${10 + depth * 14}px`,
                   onclick: can ? () => toggle(st.open, p) : null },
          el("span", { className: "tw", textContent: can ? (open ? "▾" : "▸") : "" }), o.name),
        // (Hover a value: who last wrote it, and what it was before; the
        // row's "writes": its writes. Only a value in memory: not an
        // argument, which is in its call's record, at 0.)
        o.addr && o.size
          ? el("td", { className: "fval mem", title: value, onmouseenter: (ev) => (ev.target.title = `${value}\n${wroteIt(o)}`) }, value)
          : el("td", { className: "fval", title: value }, value),
        el("td", { className: "ftype" }, o.ty, hasWrites(o, life)
          ? el("a", { className: `hist${st.history.has(p) ? " on" : ""}`, textContent: "writes", title: st.history.has(p) ? "fold its writes" : "its writes, each a link to its moment",
                      onclick: () => toggle(st.history, p) }) : null)));
      if (st.history.has(p) && hasWrites(o, life)) out.push(history(o, p, life, depth));
      // (A pointer's target is another object: its own lifetime.)
      if (open) rows(kids(o) ?? [], p, depth + 1, out, o.target != null ? null : life, o.target != null ? `${e}->` : `${e}.`);
    }
  }
  // Whether value `o` has writes to list: one in memory; a variable kept
  // in a wasm local, its call's (`life`).
  const hasWrites = (o, life) => (o.addr && o.size) || (o.var != null && life);
  // Value `o`'s writes (Machine.writes), a row: a local's while its frame
  // lives (`life`), another's in the whole recording; a variable in a wasm
  // local, the values its call set it to (Machine.var_writes); the write
  // in effect at the cursor marked.
  function history(o, p, life, depth) {
    const cur = app.time.cursor() ?? Infinity;
    const writes = o.var != null
      ? m.var_writes(life.from, o.var).map(([i, after], k, all) => ({ i, before: k ? all[k - 1][1] : "(uninitialized)", after }))
      : m.writes(o.addr, o.id, life?.from, life?.to, WRITES);
    const now = writes.findLastIndex((x) => x.i <= cur);
    const items = writes.map((x, j) => el("li", { className: j === now ? "on" : "" },
      el("a", { textContent: `${x.member ? `${x.member}: ` : ""}${x.before} → ${x.after}`, title: "go to this write", onclick: () => app.time.go(x.i) }),
      x.at ? el("span", { className: "dim", textContent: ` in ${x.at.func}` }) : null));
    return el("tr", { className: "history" }, el("td", { colSpan: 3, style: `padding-left:${24 + depth * 14}px` },
      el("div", { className: "dim" }, writes.length ? `${writes.length}${writes.length >= WRITES ? "+" : ""} write${writes.length === 1 ? "" : "s"} ${life?.what ?? "in the recording"}` : `not written ${life?.what ?? "in the recording"}`),
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
  return { rows, kids, moved };
}

// Whether value `o` opens: into its fields, or what it points to.
export const openable = (o) => o.children || o.expand != null || (o.target != null && o.value !== "NULL");

// Where a write was: "ksys_read fs/read_write.c:720".
export const place = (at) => (at ? `${at.inlined ?? at.func} ${at.file}:${at.line}` : "?");
