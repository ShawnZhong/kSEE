// The Objects: what the selected call (a syscall, or a call double-clicked
// in the timeline: the zoom) did to the kernel's data (Machine.objects):
// a box per object it changed that is still there when it returns (named
// as the code names them: "vfs_read's file", "prog(19)->files"), each
// member it changed from what it was to what it is as the call ends
// ("f_pos  0 → 18"; one left as it was, a count taken and dropped, dim),
// and who: the function, or a line each for the ones that changed it in
// turn (click: its write). As a tree: each box under the first that
// points to it, by that member ("← f_inode"). The same at every moment:
// the moments are the Stack's.
import { $, el } from "./dom.js";

export function objects(app) {
  const { m } = app;
  const body = $("objects");
  let shown = null, selected = null;
  // (The members whose runs are shown in full, past six, by box and
  // member: "3/0"; for the selection.)
  const open = new Set();

  // Draw for the selection (only while the tab is shown; again for another
  // selection or recording).
  function draw() {
    if ($("right").dataset.tab !== "objects") return;
    const w = app.selection();
    const n = m.records(), key = w ? `${w.key}:${n ? m.record_id(n - 1) : ""}` : null;
    if (key !== shown) {
      if (w?.key !== selected) (open.clear(), (selected = w?.key));
      shown = key;
      $("objects-note").textContent = w ? "" : "Select a call (click one in the program, or a syscall in the timeline): what it did to the kernel's data.";
      const top = body.scrollTop;
      body.replaceChildren(...(w ? render(m.objects(w.a, w.b)).filter(Boolean) : []));
      body.scrollTop = top;
    }
    mark(app.time.cursor());
  }

  function render(nodes) {
    if (!nodes.length) return [el("div", { className: "dim head" }, "It changed nothing that is still there when it returns (its own locals: the Stack).")];
    // (As a tree: each box under the one that points to it (Node.under),
    // from the ones nothing points to; siblings in order.)
    const order = [];
    const visit = (k, depth) => (order.push([k, depth]), nodes.forEach((n, c) => n.under?.[0] === k && visit(c, depth + 1)));
    nodes.forEach((n, k) => n.under == null && visit(k, 0));
    return [el("div", { className: "dim head" }, `${nodes.length} object${nodes.length === 1 ? "" : "s"}`),
      ...order.map(([k, depth]) => Object.assign(box(nodes[k], nodes, k), { style: `margin-left: ${Math.min(depth, 6) * 14}px` }))];
  }

  // An object's box: its name, its type, the member it hangs from; each
  // member changed, before → after (a pointer to another box: that box, a
  // link to it), and who changed it: the function, or, when more did, a
  // line each, in turn (past six, the others on asking).
  function box(n, nodes, k) {
    const rows = [];
    n.changes.forEach((d, j) => {
      const to = d.to != null ? nodes[d.to] : null, runs = d.runs, one = runs.length === 1;
      const change = (a, b, last) => [el("span", { className: "dim" }, `${a} → `), last && to ? el("a", { textContent: to.what, onclick: (e) => (e.stopPropagation(), flash(d.to)) }) : b];
      const go = (r) => ({ dataset: { last: r.last }, onclick: () => app.time.go(r.last),
        title: `${r.writes === 1 ? "written once" : `written ${r.writes} times`}${r.at ? ` in ${r.at.func} (${r.at.file}:${r.at.line})` : ""}: click to go there` });
      rows.push(el("tr", { className: d.before === d.after ? "same" : "", ...go(runs.at(-1)) },
        el("td", { className: "fname" }, d.member || "(itself)"),
        el("td", { className: "fval" }, ...change(d.before, d.after, true)),
        el("td", { className: "ftype" }, one ? runs[0].at?.func ?? "" : "")));
      if (one) return;
      const p = `${k}/${j}`, all = open.has(p) || runs.length <= 6;
      for (const [r, q] of (all ? runs : runs.slice(0, 5)).map((r, q) => [r, q])) {
        rows.push(el("tr", { className: "run", ...go(r) },
          el("td", { className: "fname" }, r.at?.func ?? "?"),
          el("td", { className: "fval" }, ...change(r.before, r.after, q === runs.length - 1)), el("td")));
      }
      if (!all) rows.push(el("tr", { className: "run" }, el("td", { colSpan: 3 },
        el("a", { textContent: `▸ ${runs.length - 5} more`, onclick: () => (open.add(p), (shown = null), draw()) }))));
    });
    return el("div", { className: `obj ${n.kind}`, dataset: { k } },
      el("div", { className: "title" }, el("b", { textContent: n.what }), el("span", { className: "dim", textContent: ` ${n.ty}${n.under ? `  ← ${n.under[1]}` : ""}` })),
      el("table", { className: "objs" }, el("tbody", {}, ...rows)));
  }

  // The change the cursor is at (its last write): marked.
  function mark(cur) {
    for (const r of body.querySelectorAll("tr[data-last]")) r.classList.toggle("on", Number(r.dataset.last) === cur);
  }

  // Go to object `k`'s box: in view, a flash.
  function flash(k) {
    const b = body.querySelector(`.obj[data-k="${k}"]`);
    if (!b) return;
    b.scrollIntoView({ block: "nearest", behavior: "smooth" });
    b.classList.remove("flash");
    void b.offsetWidth; // (again, if it was flashing)
    b.classList.add("flash");
  }

  return { draw, mark: (i) => $("right").dataset.tab === "objects" && mark(i ?? app.time.cursor()) };
}
