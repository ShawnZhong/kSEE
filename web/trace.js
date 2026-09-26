// The Trace: the selected call (a syscall, or a call clicked in the
// timeline) as a tree of the calls it made, in order, pruned to what
// matters (Machine.call_tree: not too deep, not too small; "+n small
// calls" for the rest), each a link to its first record. A call opens
// (▸) to its own records (Machine.raw: blocks, arguments, stores, returns,
// as the recording has them), its callees in their places, and (changed)
// to what it changed (Machine.changes: by object, each said as what it is, each write
// a link; click an object to watch it). On top, where its records went, by
// area (as the timeline colors them). The cursor's record is marked, and
// the view follows the pointer over a record (app.peek).
import { $, el } from "./dom.js";
import { place } from "./objects.js";
import { AREAS, color } from "./timeline.js";

const DEPTH = 6, MIN = 20, ROWS = 400;

export function trace(app, watch) {
  const { m } = app;
  const funcs = new Map();
  const fn = (id) => (funcs.has(id) ? funcs.get(id) : funcs.set(id, m.function(id)).get(id));
  const body = $("trace");
  body.onpointerleave = () => app.peek(null);
  // What is open: nodes (by first record) and their changes.
  const open = new Set(), changed = new Set();
  let shown = null, root = null, total = 1;

  // Draw for the selection (again only if it, or the snapshot, changed).
  function draw() {
    const w = app.time.home();
    const key = w ? `${w.a}:${w.b}:${w.task}:${m.record_id(m.records() - 1)}` : null;
    if (key === shown) return;
    shown = key;
    funcs.clear();
    open.clear();
    changed.clear();
    $("trace-note").textContent = w ? "" : "Select a call (click one in the program, or a syscall in the timeline): what it did, as a tree; open a call for its records and what it changed.";
    if (!w) return body.replaceChildren();
    const task = w.task ?? m.at(w.a)?.task.addr ?? 0;
    const calls = m.call_tree(w.a, w.b, task, DEPTH, MIN);
    const at = m.at(w.a);
    root = { func: null, name: at?.syscall != null ? `${app.calls()[at.syscall]?.name ?? "?"}()` : at?.func ?? "?", first: w.a, last: w.b, records: w.b - w.a + 1, small: 0, calls };
    total = root.records;
    // Where the records went, by area: the records of each call, by its function's file.
    const by = new Map();
    const tally = (nodes) => nodes.forEach((n) => {
      const own = n.records - n.calls.reduce((a, x) => a + x.records, 0);
      const area = AREAS.find(([p]) => fn(n.func)?.file?.startsWith(p))?.[2] ?? "other";
      by.set(area, (by.get(area) ?? 0) + own);
      tally(n.calls);
    });
    tally(calls);
    const bar = el("div", { className: "areas", title: "where the records went: by the area of the function running" },
      ...[...by].sort((a, b) => b[1] - a[1]).map(([area, n]) => el("span", { className: "area", style: `flex:${n};background:${color(AREAS.find(([, , a]) => a === area)?.[0], false)}`,
        title: `${area}: ${Math.round((100 * n) / total)}%`, textContent: n / total > 0.12 ? area : "" })));
    body.replaceChildren(bar, el("ul", { className: "tree" }, item(root)));
    mark();
  }

  // A call as a list item: its header; open, what it changed (if asked),
  // and its records with its callees in their places.
  function item(n) {
    const f = n.func == null ? null : fn(n.func), name = n.name ?? f?.name ?? "?";
    const pct = Math.round((100 * n.records) / total);
    const li = el("li", { dataset: { first: n.first } });
    const redo = () => li.replaceWith(item(n));
    const isOpen = open.has(n.first);
    li.append(el("div", { className: "call" },
      el("a", { className: "tw", textContent: isOpen ? "▾" : "▸", onclick: () => (isOpen ? open.delete(n.first) : open.add(n.first), redo()) }),
      el("a", { className: "mono", textContent: name, title: `${f?.file ?? ""}:${f?.line ?? ""}: go to this call`, onclick: () => app.time.go(n.first) }),
      el("span", { className: "dim", textContent: ` ${pct >= 1 ? `${pct}%` : `${n.records} records`}` }),
      el("a", { className: "dim what", textContent: changed.has(n.first) ? " changed ▾" : " changed ▸", title: "what it changed, by object",
                onclick: () => (changed.has(n.first) ? changed.delete(n.first) : changed.add(n.first), redo()) })));
    if (changed.has(n.first)) li.append(changes(n));
    if (n.calls.length || n.small || isOpen) li.append(records(n, isOpen));
    return li;
  }

  // A call's callees (each an item), the small ones counted; open, its
  // own records too, in their places.
  function records(n, isOpen) {
    const ul = el("ul", { className: "recs" });
    let at = n.first, rows = 0;
    const lines = (from, to) => {
      if (!isOpen || to < from || rows >= ROWS) return;
      for (const [i, text] of m.raw(from, to)) {
        if (rows++ >= ROWS) return ul.append(el("li", { className: "dim small", textContent: `… ${to - i + 1} more records` }));
        const kind = text.startsWith("──") ? "task" : text.split(" ")[0];
        ul.append(el("li", { className: `rec ${kind}`, dataset: { i }, onclick: () => app.time.go(i), onpointerenter: () => app.peek(i) },
          el("span", { className: "dim idx", textContent: i }), " ", text));
      }
    };
    for (const c of n.calls) {
      lines(at, c.first - 1);
      ul.append(item(c));
      at = c.last + 1;
    }
    lines(at, n.last);
    if (n.small) ul.append(el("li", { className: "dim small", textContent: `+${n.small} small call${n.small > 1 ? "s" : ""} not listed` }));
    return ul;
  }

  // What call `n` changed, by object (Machine.changes over its records).
  function changes(n) {
    const cur = app.time.cursor() ?? Infinity;
    const list = m.changes(n.first, n.last);
    const objects = list.filter((c) => c.addr);
    return el("div", { className: "changes" },
      el("div", { className: "dim note", textContent: objects.length ? `${list.reduce((a, c) => a + c.n, 0)} writes to ${objects.length} objects` : "no writes" }),
      ...list.map((c) => {
        if (!c.addr) return el("div", { className: "dim note", textContent: `and ${c.n} stores to memory no call here got by argument or local` });
        const now = c.writes.findLastIndex((x) => x.i <= cur);
        return el("div", { className: "changed" },
          el("div", { className: "whead" },
            // (Watched as what it is: "f's x", or "*(struct file *)0x1234"; a
            // local or a frame for as long as its call lives.)
            el("a", { className: "mono", textContent: c.what, title: "watch it",
                      onclick: () => watch({ name: /^(struct|union) /.test(c.what) ? `*(${c.ty} *)0x${c.addr.toString(16)}` : c.what, ty: c.ty, id: c.id, addr: c.addr },
                                           c.call == null ? null : { from: c.call, to: m.call_end(c.call) ?? undefined, what: `while ${c.what.split("'s ")[0]}'s frame lives` }) }),
            el("span", { className: "dim", textContent: ` ${c.n} write${c.n === 1 ? "" : "s"}${c.n > c.writes.length ? `, the last ${c.writes.length}` : ""}` })),
          el("ol", {}, ...c.writes.map((x, j) => el("li", { className: j === now ? "on" : "" },
            el("a", { textContent: `${x.member ? `${x.member}: ` : ""}${x.before} → ${x.after}`, onclick: () => app.time.go(x.i), onpointerenter: () => app.peek(x.i) }),
            el("span", { className: "dim", textContent: ` ${place(x.at)}` })))));
      }));
  }

  // The cursor moved: its record marked, if shown.
  function mark() {
    const cur = app.time.cursor();
    body.querySelector("li.rec.on")?.classList.remove("on");
    const li = cur == null ? null : body.querySelector(`li.rec[data-i="${cur}"]`);
    if (li) {
      li.classList.add("on");
      if ($("right").dataset.tab === "trace") li.scrollIntoView({ block: "nearest" });
    }
  }
  return { draw, mark };
}
