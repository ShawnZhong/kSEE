// The Trace: the selected call (a syscall, or a call clicked in the
// timeline) as a tree of the calls it made, in order (Machine.call_tree),
// pruned to what matters: calls of few records, or deep down, are counted
// as "+n small calls", which open on a click; "all" shows every call; ▾
// folds a call's calls away. Each call is a link to its first record; its
// share of the records is beside it, and the call the cursor is in is
// marked. What a syscall did, readable.
import { $, el } from "./dom.js";

// Calls deeper than DEPTH, or of fewer than MIN records, are small.
const DEPTH = 6, MIN = 20;

export function trace(app) {
  const { m } = app;
  const funcs = new Map();
  const fn = (id) => (funcs.has(id) ? funcs.get(id) : funcs.set(id, m.function(id)).get(id));
  const body = $("trace");
  // The calls whose small calls are opened, and the calls folded (by their
  // first record); or all.
  const opened = new Set(), folded = new Set();
  let shown = null, tree = [], total = 1, all = false, count = 0;

  // Draw for the selection (again only if it, or the snapshot, changed).
  function draw() {
    const w = app.time.home();
    const key = w ? `${w.a}:${w.b}:${w.task}:${m.record_id(m.records() - 1)}` : null;
    if (key === shown) return;
    shown = key;
    funcs.clear();
    opened.clear();
    folded.clear();
    $("trace-note").textContent = w ? "" : "Select a call (click one in the program, or a syscall in the timeline): what it did, as a tree of calls.";
    if (!w) return body.replaceChildren();
    const task = w.task ?? m.at(w.a)?.task.addr ?? 0;
    tree = m.call_tree(w.a, w.b, task, 1e9, 1);
    total = w.b - w.a + 1;
    const n = (nodes) => nodes.reduce((a, x) => a + 1 + n(x.calls), 0);
    count = n(tree);
    render();
  }

  function render() {
    // (The head: how many calls, and the main ones or all of them.)
    const head = el("div", { className: "dim head" }, `${count} call${count === 1 ? "" : "s"} · `,
      el("a", { textContent: all ? "the main ones" : "all", title: all ? "the calls that matter: the rest as +n small calls" : "every call, small ones too",
                onclick: () => ((all = !all), render()) }));
    body.replaceChildren(head, el("ul", { className: "tree" }, ...list(tree, 0)));
    mark();
  }

  // The items for `nodes` (calls in order) at `depth`: the ones that matter,
  // then, for the small ones, "+n small calls" (opened: them too).
  function list(nodes, depth) {
    const small = (n) => !all && (depth >= DEPTH || n.records < MIN);
    const items = [], rest = nodes.filter(small);
    for (const n of nodes) if (!small(n)) items.push(item(n, depth));
    if (rest.length) {
      const key = `${nodes[0].first}:${depth}`;
      if (opened.has(key)) items.push(...rest.map((n) => item(n, depth, true)));
      else items.push(el("li", { className: "small" }, el("a", { className: "dim", textContent: `+${rest.length} small call${rest.length > 1 ? "s" : ""}`,
        title: "the calls of few records here: open them", onclick: () => (opened.add(key), render()) })));
    }
    return items;
  }
  function item(n, depth, small = false) {
    const f = fn(n.func), pct = Math.round((100 * n.records) / total), shut = folded.has(n.first);
    return el("li", { className: small ? "small" : "", dataset: { first: n.first, last: n.last } },
      el("a", { className: "tw", textContent: n.calls.length ? (shut ? "▸" : "▾") : "", title: shut ? "its calls" : "fold its calls away",
                onclick: () => (shut ? folded.delete(n.first) : folded.add(n.first), render()) }),
      el("a", { className: "mono", textContent: f?.name ?? "?", title: `${f?.file ?? ""}:${f?.line ?? ""}: go to this call`, onclick: () => app.time.go(n.first) }),
      el("span", { className: "dim", textContent: pct >= 1 ? ` ${pct}%` : ` ${n.records}` }),
      n.calls.length && !shut ? el("ul", {}, ...list(n.calls, depth + 1)) : null);
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
