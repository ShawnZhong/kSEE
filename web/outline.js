// The Outline: the selected call (a syscall) as a tree of the calls it
// made, in order, pruned to what matters (Machine.call_tree: not too deep,
// not too small; "+n small calls" for the rest), each a link to its first
// record; on top, where its records went, by area (as the timeline colors
// them). What a syscall did, readable.
import { $, el } from "./dom.js";
import { AREAS, color } from "./timeline.js";

export function outline(app) {
  const { m } = app;
  const DEPTH = 6, MIN = 20;
  const funcs = new Map();
  const fn = (id) => (funcs.has(id) ? funcs.get(id) : funcs.set(id, m.function(id)).get(id));
  // (A tab of the right column, made here: its pane after the others.)
  const pane = el("div", { className: "pane", dataset: { tab: "outline" } },
    el("div", { className: "stackhead dim", id: "outline-note" }), el("div", { className: "body", id: "outline" }));
  $("right").append(pane);
  document.querySelector("h2.tabs").append(el("a", { dataset: { tab: "outline" }, title: "the selected call as a tree of the calls it made, pruned", textContent: "Outline",
    onclick: () => ($("right").dataset.tab = "outline") }));

  let shown = null; // (the syscall's index, and the newest record's id: a new snapshot moves indexes)
  function draw() {
    const w = app.time.home(), calls = app.calls();
    const c = w && calls.find((x) => x.first === w.a && x.task.addr === w.task);
    const key = c ? `${c.index}:${m.record_id(m.records() - 1)}` : null;
    if (key === shown) return;
    shown = key;
    funcs.clear();
    $("outline-note").textContent = c ? "" : "Select a call (click one in the program, or a syscall in the timeline): what it did, as a tree.";
    if (!c) return $("outline").replaceChildren();
    const tree = m.call_tree(c.index, DEPTH, MIN), total = (c.last ?? m.records() - 1) - c.first + 1;
    // Where the records went, by area: the records of each call, by its function's file.
    const by = new Map();
    const tally = (nodes) => nodes.forEach((n) => {
      const own = n.records - n.calls.reduce((a, x) => a + x.records, 0);
      const area = AREAS.find(([p]) => fn(n.func)?.file?.startsWith(p))?.[2] ?? "other";
      by.set(area, (by.get(area) ?? 0) + own);
      tally(n.calls);
    });
    tally(tree);
    const bar = el("div", { className: "areas", title: "where the records went: by the area of the function running" },
      ...[...by].sort((a, b) => b[1] - a[1]).map(([area, n]) => el("span", { className: "area", style: `flex:${n};background:${color(AREAS.find(([, , a]) => a === area)?.[0], false)}`,
        title: `${area}: ${Math.round((100 * n) / total)}%`, textContent: n / total > 0.12 ? area : "" })));
    const item = (n) => {
      const f = fn(n.func), pct = Math.round((100 * n.records) / total);
      return el("li", {},
        el("a", { className: "mono", textContent: f?.name ?? "?", title: `${f?.file ?? ""}:${f?.line ?? ""}: go to this call`, onclick: () => app.time.go(n.first) }),
        el("span", { className: "dim", textContent: pct >= 1 ? ` ${pct}%` : "" }),
        n.calls.length || n.small ? el("ul", {}, ...n.calls.map(item), n.small ? el("li", { className: "dim small", textContent: `+${n.small} small call${n.small > 1 ? "s" : ""}` }) : null) : null);
    };
    $("outline").replaceChildren(bar, el("ul", { className: "tree" }, ...tree.map(item)));
  }
  return { draw };
}
