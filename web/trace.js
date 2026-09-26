// The Trace: the selected call (a syscall, or a call clicked in the
// timeline) as a tree of every call it made, in order (Machine.call_tree);
// click a call to go to it and open its callees (▸ opens and closes them;
// a chain of single calls opens by itself). "records" shows a call's own
// records (Machine.raw: calls with their arguments, blocks, stores,
// returns, as the recording has them) in among its callees; "changed"
// what it changed (Machine.changes: by object, each said as what it is,
// each write a link; click an object to watch it). The cursor's record is
// marked, and the view follows the pointer over a record (app.peek).
import { $, el } from "./dom.js";
import { place } from "./objects.js";

const ROWS = 400;

export function trace(app, watch) {
  const { m } = app;
  const funcs = new Map();
  const fn = (id) => (funcs.has(id) ? funcs.get(id) : funcs.set(id, m.function(id)).get(id));
  const body = $("trace");
  body.onpointerleave = () => app.peek(null);
  // What is open (by the call's first record): its callees, its records, its changes.
  const open = new Set(), recs = new Set(), changed = new Set();
  let shown = null, total = 1;

  // Draw for the selection (again only if it, or the snapshot, changed).
  function draw() {
    const w = app.time.home();
    const key = w ? `${w.a}:${w.b}:${w.task}:${m.record_id(m.records() - 1)}` : null;
    if (key === shown) return;
    shown = key;
    funcs.clear();
    open.clear();
    recs.clear();
    changed.clear();
    $("trace-note").textContent = w ? "" : "Select a call (click one in the program, or a syscall in the timeline): what it did, as a tree; open a call for its records and what it changed.";
    if (!w) return body.replaceChildren();
    const task = w.task ?? m.at(w.a)?.task.addr ?? 0;
    const calls = m.call_tree(w.a, w.b, task, 1e9, 1);
    const at = m.at(w.a);
    const root = { func: null, name: at?.syscall != null ? `${app.calls()[at.syscall]?.name ?? "?"}()` : at?.func ?? "?", first: w.a, last: w.b, records: w.b - w.a + 1, calls };
    total = root.records;
    // (The root open, and on down a chain of single calls.)
    for (let n = root; n; n = n.calls.length === 1 ? n.calls[0] : null) open.add(n.first);
    body.replaceChildren(el("ul", { className: "tree" }, item(root)));
    mark();
  }

  // A call as a list item: its header; open, what it changed (if asked),
  // and its records with its callees in their places.
  function item(n) {
    const f = n.func == null ? null : fn(n.func), name = n.name ?? f?.name ?? "?";
    const pct = Math.round((100 * n.records) / total);
    const li = el("li", { dataset: { first: n.first } });
    const redo = () => li.replaceWith(item(n));
    const isOpen = open.has(n.first), showRecs = recs.has(n.first);
    const toggle = (set, text, title) => el("a", { className: "dim what", textContent: ` ${text} ${set.has(n.first) ? "▾" : "▸"}`, title,
                                              onclick: () => (set.has(n.first) ? set.delete(n.first) : set.add(n.first), redo()) });
    li.append(el("div", { className: "call" },
      el("a", { className: "tw", textContent: n.calls.length ? (isOpen ? "▾" : "▸") : "", onclick: () => (isOpen ? open.delete(n.first) : open.add(n.first), redo()) }),
      // (The name: go to the call, and open its callees.)
      el("a", { className: "mono", textContent: name, title: `${f?.file ?? ""}:${f?.line ?? ""}: go to this call, and open its calls`,
                onclick: () => (app.time.go(n.first), n.calls.length && !isOpen && (open.add(n.first), redo())) }),
      el("span", { className: "dim", textContent: ` ${pct >= 1 ? `${pct}% of the records` : `${n.records} records`}${n.calls.length ? `, ${n.calls.length} call${n.calls.length > 1 ? "s" : ""}` : ""}` }),
      el("span", { className: "dim", textContent: " ·" }), toggle(recs, "records", "its own records, in among its calls"),
      el("span", { className: "dim", textContent: " ·" }), toggle(changed, "changed", "what it changed, by object")));
    if (changed.has(n.first)) li.append(changes(n));
    if ((n.calls.length && isOpen) || showRecs) li.append(records(n, showRecs));
    return li;
  }

  // A call's callees (each an item); with `showRecs`, its own records
  // too, in their places.
  function records(n, showRecs) {
    const ul = el("ul", { className: "recs" });
    let at = n.first, rows = 0;
    const lines = (from, to) => {
      if (!showRecs || to < from || rows >= ROWS) return;
      for (const [i, text] of m.raw(from, to)) {
        if (rows++ >= ROWS) return ul.append(el("li", { className: "dim small", textContent: `… ${to - i + 1} more records` }));
        const kind = text.startsWith("──") ? "task" : text.split(" ")[0]; // (call, block, store, return)
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
