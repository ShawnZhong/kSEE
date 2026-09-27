// The Calls: the selected call (a syscall, or a call clicked in the
// timeline) as a tree of every call it made, in the order they were made,
// with the code inlined into a call as calls of its own, as the timeline
// and the Stack show it (Machine.call_tree). Its direct calls to begin
// with (and on down a chain of single calls); ▸ opens a call's own calls,
// for the detail; the head opens all, closes all, or opens the way to the
// call the cursor is in ("here"). Each call is a link to
// its first record (hover: how many records it took), and the call the
// cursor is in is marked. Inlined code that only inlines one more
// function is one row with it (set_bit → arch_set_bit → …: the kernel's
// layers of helpers). What a syscall did, readable.
import { $, el } from "./dom.js";
import { result } from "./args.js";

export function trace(app) {
  const { m } = app;
  const funcs = new Map(), inames = new Map();
  const fn = (id) => (funcs.has(id) ? funcs.get(id) : funcs.set(id, m.function(id)).get(id));
  const iname = (k) => inames.get(k) ?? inames.set(k, m.inline_name(k)).get(k);
  const body = $("trace");
  // The calls opened (by their first record): their calls shown. (A chain
  // of single calls opens by itself: the spine of a syscall shows at once.)
  const opened = new Set();
  let shown = null, tree = [], total = 1, count = 0;
  const consts = m.constants();

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
    for (let nodes = tree; nodes.length === 1 && nodes[0].calls.length; nodes = nodes[0].calls) opened.add(nodes[0].first);
    render();
  }

  // Every call with calls, and the ones on the way to record `i`.
  const withCalls = (nodes, out = []) => (nodes.forEach((n) => n.calls.length && (out.push(n.first), withCalls(n.calls, out))), out);
  const wayTo = (nodes, i, out = []) => {
    const n = nodes.find((n) => i >= n.first && i <= n.last);
    return n ? (n.calls.length && out.push(n.first), wayTo(n.calls, i, out)) : out;
  };
  function render() {
    const verb = (text, title, act) => el("a", { textContent: text, title, onclick: () => (act(), render()) });
    body.replaceChildren(el("div", { className: "dim head" }, `${count} call${count === 1 ? "" : "s"} · `,
      verb("open all", "every call's calls shown", () => withCalls(tree).forEach((f) => opened.add(f))), " · ",
      verb("close all", "the direct calls only", () => opened.clear()), " · ",
      verb("here", "the way to the call the cursor is in, and no more", () => (opened.clear(), wayTo(tree, app.time.cursor() ?? -1).forEach((f) => opened.add(f))))),
      el("ul", { className: "tree" }, ...tree.map(item)));
    mark();
  }

  // A call: ▸ for its calls, its name (a link; inlined code in italics),
  // and, inlined code with one inlined call and nothing else, that call
  // on the same row (→), down the chain.
  function item(n) {
    const names = [...name(n)];
    let last = n;
    while (last.inlined && last.calls.length === 1 && last.calls[0].inlined) {
      last = last.calls[0];
      names.push(" → ", ...name(last));
    }
    const open = opened.has(last.first);
    return el("li", { dataset: { first: n.first, last: n.last } },
      el("a", { className: "tw", textContent: last.calls.length ? (open ? "▾" : "▸") : "", title: open ? "fold its calls away" : "its calls",
                onclick: () => (open ? opened.delete(last.first) : opened.add(last.first), render()) }),
      ...names, last.calls.length && open ? el("ul", {}, ...last.calls.map(item)) : null);
  }
  // (The name, a link; and what it returned: "= 26", "= -2 (ENOENT)".)
  function name(n) {
    const f = n.inlined ? null : fn(n.func), pct = Math.round((100 * n.records) / total);
    const a = el("a", { className: n.inlined ? "mono inl" : "mono", textContent: n.inlined ? iname(n.func) : f?.name ?? "?", onclick: () => app.goCall(n.first),
      title: `${n.inlined ? "inlined code (no call): " : `${f?.file ?? ""}:${f?.line ?? ""}: `}${n.records} record${n.records === 1 ? "" : "s"}${pct >= 1 ? ` (${pct}% of the selection)` : ""}, ${n.calls.length} call${n.calls.length === 1 ? "" : "s"}; go there` });
    return n.ret == null ? [a] : [a, el("span", { className: "ret mono", textContent: ` = ${result(n.ret, consts)}` })];
  }

  // The cursor moved: the innermost call shown that it is in, marked.
  function mark(i) {
    const cur = i ?? app.time.cursor();
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
