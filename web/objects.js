// The Objects: what the selected call (a syscall, or a call double-clicked
// in the timeline: the zoom) did to the kernel's data (Machine.objects):
// a box per object it changed that is still there when it returns (named
// as the code names them: "file in vfs_read", "prog(19)->files"), each
// member it changed, a line a function that changed it in turn, from
// what to what ("f_mode  0 → 0x4000001  init_file", then "0x4000001 →
// 0x40a801d  do_dentry_open"; one left as it was, a count one took and
// another dropped, dim; click: that write). As a tree: each box under the first that
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
  // member changed, a line a function that changed it in turn, from what
  // to what (a pointer to another box, at the end: that box, a link to
  // it); past six, the others on asking. One left as it was, dim.
  function box(n, nodes, k) {
    const rows = [];
    n.changes.forEach((d, j) => {
      const to = d.to != null ? nodes[d.to] : null, runs = d.runs, p = `${k}/${j}`;
      const list = open.has(p) || runs.length <= 6 ? runs : runs.slice(0, 5);
      list.forEach((r, q) => rows.push(el("tr", { className: d.before === d.after ? "same" : "", dataset: { last: r.last }, onclick: () => app.time.go(r.last),
          title: `${r.writes === 1 ? "written once" : `written ${r.writes} times`}${r.at ? ` in ${r.at.func} (${r.at.file}:${r.at.line})` : ""}: click to go there` },
        el("td", { className: "fname" }, q ? "" : d.member || "(itself)"),
        el("td", { className: "fval" }, el("span", { className: "dim" }, `${r.before} → `),
          q === runs.length - 1 && to ? el("a", { textContent: to.what, onclick: (e) => (e.stopPropagation(), flash(d.to)) }) : r.after),
        el("td", { className: "ftype" }, r.at?.func ?? ""))));
      if (list.length < runs.length) rows.push(el("tr", {}, el("td"), el("td", { colSpan: 2 },
        el("a", { textContent: `▸ ${runs.length - list.length} more`, onclick: () => (open.add(p), (shown = null), draw()) }))));
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
