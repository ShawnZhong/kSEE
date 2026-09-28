// Values as trees (the Stack's): a row a value, its name, value and type;
// pointers open into what they point to, structs into their fields, read
// from memory as it was at the cursor (Machine.memory_at). Values that
// changed since the cursor last moved are highlighted; hover one in memory
// for who last wrote it; click it for its writes (Machine.writes), a row
// under it: each the member, from what to what, in which function, a link
// to that moment, the one in effect at the cursor marked; a local's writes
// while its frame lives, another object's within the selected call ("all"
// for the whole recording); click the value again to close.
import { el } from "./dom.js";

// The most writes of a value shown (the newest).
const WRITES = 256;

// `st`: a tree's state: { open (paths open), history (paths whose writes
// are shown), wide (of those, the ones over the whole recording), last,
// now (values shown before and now, by path) }; `redraw()`: draw it again
// (a value opened or closed).
export function tree(app, st, redraw) {
  const { m } = app;
  st.history ??= new Set();
  st.wide ??= new Set();
  const toggle = (set, p) => (set.has(p) ? set.delete(p) : set.add(p), redraw());
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
        // its writes. Only a value in memory: not an argument, which is in
        // its call's record, at 0.)
        o.addr && o.size
          ? el("td", { className: "fval mem", title: value, onmouseenter: (ev) => (ev.target.title = `${value}\n${wroteIt(o)}\n(click: its writes)`),
                       onclick: () => toggle(st.history, p) }, value)
          : el("td", { className: "fval", title: value }, value),
        el("td", { className: "ftype", textContent: o.ty })));
      if (st.history.has(p)) out.push(history(o, p, life, depth));
      // (A pointer's target is another object: its own lifetime.)
      if (open) rows(kids(o) ?? [], p, depth + 1, out, o.target != null ? null : life, o.target != null ? `${e}->` : `${e}.`);
    }
  }
  // Value `o`'s writes (Machine.writes), a row: a local's while its frame
  // lives (`life`), another object's within the selected call, or, wide,
  // in the whole recording; the write in effect at the cursor marked.
  function history(o, p, life, depth) {
    const cur = app.time.cursor() ?? Infinity, wide = st.wide.has(p);
    const home = app.time.home();
    const span = wide ? null : life ?? (home ? { from: home.a, to: home.b, what: "in this call" } : null);
    const writes = m.writes(o.addr, o.id, span?.from, span?.to, WRITES);
    const now = writes.findLastIndex((x) => x.i <= cur);
    const items = writes.map((x, j) => el("li", { className: j === now ? "on" : "" },
      el("a", { textContent: `${x.member ? `${x.member}: ` : ""}${x.before} → ${x.after}`, title: "go to this write", onclick: () => app.time.go(x.i) }),
      el("span", { className: "dim", textContent: ` in ${x.at?.func ?? "?"}` })));
    const scope = span ? span.what : "in the recording";
    return el("tr", { className: "history" }, el("td", { colSpan: 3, style: `padding-left:${24 + depth * 14}px` },
      el("div", { className: "dim" }, `${writes.length}${writes.length >= WRITES ? "+" : ""} write${writes.length === 1 ? "" : "s"} ${scope}`,
        span ? el("a", { textContent: " · all", title: "in the whole recording", onclick: () => toggle(st.wide, p) })
          : life || home ? el("a", { textContent: " · fewer", title: span ? "" : "within the frame, or the call", onclick: () => toggle(st.wide, p) }) : null),
      items.length ? el("ol", {}, ...items) : null));
  }

  // The newest write to `o` at the cursor (Machine.last_write): where, by
  // whom, and the value it overwrote.
  // Who last wrote value `o`, and where from: back through the copies
  // that brought it there (Machine.origin), a line each.
  function wroteIt(o) {
    const way = m.origin(o.addr, Math.min(o.size, 8));
    if (!way.length) return "not written in the recording";
    const [w, ...back] = way, last = way.at(-1);
    return [`written at ${place(w.at)}, by ${w.task.comm ?? "?"}(${w.task.pid ?? "?"}); was ${w.old}`,
      ...back.map((x) => `  ← copied from 0x${x.addr.toString(16)}, written at ${place(x.at)}`),
      ...(last.from != null ? [`  ← copied from 0x${last.from.toString(16)}, not written in the recording`] : [])].join("\n");
  }
  return { rows, kids };
}

// Where a write was: "ksys_read fs/read_write.c:720".
export const place = (at) => (at ? `${at.inlined ?? at.func} ${at.file}:${at.line}` : "?");
