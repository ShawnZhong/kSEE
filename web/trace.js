// The Calls: the selected call (a syscall, or a call clicked in the
// timeline) as a tree of every call it made, in the order they were made,
// with the code inlined into a call as calls of its own, as the timeline
// and the Stack show it (Machine.call_tree). Its direct calls to begin
// with; ▸ opens a call's own calls, for the detail. Each call is a link to
// its first record (hover: how many records it took), and the call the
// cursor is in is marked. What a syscall did, readable.
import { $, el } from "./dom.js";

export function trace(app) {
  const { m } = app;
  const funcs = new Map(), inames = new Map();
  const fn = (id) => (funcs.has(id) ? funcs.get(id) : funcs.set(id, m.function(id)).get(id));
  const iname = (k) => inames.get(k) ?? inames.set(k, m.inline_name(k)).get(k);
  const body = $("trace");
  // The calls opened (by their first record): their calls shown.
  const opened = new Set();
  let shown = null, tree = [], total = 1, count = 0;

  // Draw for the selection (again only if it, or the snapshot, changed).
  function draw() {
    const w = app.time.home();
    const key = w ? `${w.a}:${w.b}:${w.task}:${m.record_id(m.records() - 1)}` : null;
    if (key === shown) return;
    shown = key;
    funcs.clear();
    inames.clear();
    opened.clear();
    $("trace-note").textContent = w ? "" : "Select a call (click one in the program, or a syscall in the timeline): the calls it made, in order.";
    if (!w) return body.replaceChildren();
    const task = w.task ?? m.at(w.a)?.task.addr ?? 0;
    tree = m.call_tree(w.a, w.b, task);
    total = w.b - w.a + 1;
    const n = (nodes) => nodes.reduce((a, x) => a + 1 + n(x.calls), 0);
    count = n(tree);
    render();
  }

  function render() {
    body.replaceChildren(el("div", { className: "dim head", textContent: `${count} call${count === 1 ? "" : "s"}` }),
      el("ul", { className: "tree" }, ...tree.map(item)));
    mark();
  }

  // A call: ▸ for its calls, its name (a link; inlined code in italics).
  function item(n) {
    const f = n.inlined ? null : fn(n.func), name = n.inlined ? iname(n.func) : f?.name ?? "?";
    const pct = Math.round((100 * n.records) / total), open = opened.has(n.first);
    return el("li", { dataset: { first: n.first, last: n.last } },
      el("a", { className: "tw", textContent: n.calls.length ? (open ? "▾" : "▸") : "", title: open ? "fold its calls away" : "its calls",
                onclick: () => (open ? opened.delete(n.first) : opened.add(n.first), render()) }),
      el("a", { className: n.inlined ? "mono inl" : "mono", textContent: name, onclick: () => app.goCall(n.first),
                title: `${n.inlined ? "inlined code (no call): " : `${f?.file ?? ""}:${f?.line ?? ""}: `}${n.records} record${n.records === 1 ? "" : "s"}${pct >= 1 ? ` (${pct}% of the selection)` : ""}, ${n.calls.length} call${n.calls.length === 1 ? "" : "s"}; go there` }),
      n.calls.length && open ? el("ul", {}, ...n.calls.map(item)) : null);
  }

  // The cursor moved: the innermost call shown that it is in, marked.
  function mark() {
    const cur = app.time.cursor();
    body.querySelector("li.on")?.classList.remove("on");
    if (cur == null) return;
    let best = null;
    for (const li of body.querySelectorAll("li[data-first]")) {
      const first = Number(li.dataset.first), last = Number(li.dataset.last);
      if (cur >= first && cur <= last && (!best || first >= Number(best.dataset.first))) best = li;
    }
    if (best) {
      best.classList.add("on");
      if ($("right").dataset.tab === "trace") best.scrollIntoView({ block: "nearest" });
    }
  }
  return { draw, mark };
}
