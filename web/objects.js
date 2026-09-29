// The Objects: what the call in focus (the syscall selected, or a call
// clicked in the timeline: app.focus) did to the kernel's data (Machine.objects):
// a box per object it changed that is still there when it returns
// (declared as the code names them: "struct file *file in vfs_read",
// "struct files_struct *prog(19)->files"; and by
// its address, which it is; a global, where it is declared), each
// member it changed, a line a function that changed it in turn, from
// what to what ("f_mode  0 → 0x4000001  init_file", then "0x4000001 →
// 0x40a801d  do_dentry_open"; one left as it was, a count one took and
// another dropped, dim; click: that write). As a tree: each box under the first that
// points to it, by that member ("← f_inode"). The whole call, at every
// moment; at the cursor (a step, the pointer over the timeline), the writes
// not made yet gray, the run of writes it is in marked.
import { $, el } from "./dom.js";

export function objects(app) {
  const { m } = app;
  const body = $("objects");
  let shown = null, selected = null;
  // (The members whose runs are shown in full, past six, by box and
  // member: "3/0"; for the selection.)
  const open = new Set();

  // Draw for the call in focus (only while the tab is shown; again for
  // another call or recording).
  function draw() {
    if ($("right").dataset.tab !== "objects") return;
    const w = app.focus();
    const n = m.records(), key = w ? `${w.key}:${n ? m.record_id(n - 1) : ""}` : null;
    if (key !== shown) {
      if (w?.key !== selected) (open.clear(), (selected = w?.key));
      shown = key;
      $("objects-note").textContent = w ? "" : "Select a call (click one in the program, or a syscall in the timeline): what it did to the kernel's data.";
      const top = body.scrollTop;
      body.replaceChildren(...(w ? render(m.objects(w.a, w.b), w).filter(Boolean) : []));
      body.scrollTop = top;
    }
    mark(app.time.cursor());
  }

  // (A call in the syscall in focus: a way back to the whole syscall.)
  function whole(w) {
    const h = app.time.home();
    return h && (h.a !== w.a || h.b !== w.b) ? ["  · ", el("a", { textContent: `the whole ${h.title}`, onclick: () => app.time.unfocus() })] : [];
  }

  function render(nodes, w) {
    const title = w.title;
    if (!nodes.length) return [el("div", { className: "dim head" }, `${title} changed nothing that outlives it`, ...whole(w))];
    // (As a tree: each box under the one that points to it (Node.under),
    // from the ones nothing points to; siblings in order.)
    const order = [];
    const visit = (k, depth) => (order.push([k, depth]), nodes.forEach((n, c) => n.under?.[0] === k && visit(c, depth + 1)));
    nodes.forEach((n, k) => n.under == null && visit(k, 0));
    return [el("div", { className: "dim head" }, `What ${title} did: ${nodes.length} object${nodes.length === 1 ? "" : "s"}`, ...whole(w)),
      // (One under another: indented, a guide line down its left.)
      ...order.map(([k, depth]) => {
        const b = box(nodes[k], nodes, k);
        if (depth) (b.classList.add("under"), (b.style.marginLeft = `${Math.min(depth, 6) * 18}px`));
        return b;
      })];
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
      list.forEach((r, q) => rows.push(el("tr", { className: d.before === d.after ? "same" : "", dataset: { first: r.first, last: r.last }, onclick: () => app.time.go(r.last),
          title: `${r.writes === 1 ? "written once" : `written ${r.writes} times`}${r.at ? ` in ${r.at.func} (${r.at.file}:${r.at.line})` : ""}: click to go there` },
        el("td", { className: "fname" }, q ? "" : d.member || "(itself)"),
        el("td", { className: "fval" }, el("span", { className: "dim" }, `${r.before} → `),
          q === runs.length - 1 && to ? el("a", { textContent: to.what, onclick: (e) => (e.stopPropagation(), flash(d.to)) }) : r.after),
        el("td", { className: "ftype" }, r.at?.func ?? ""))));
      if (list.length < runs.length) rows.push(el("tr", {}, el("td"), el("td", { colSpan: 2 },
        el("a", { textContent: `▸ ${runs.length - list.length} more`, onclick: () => (open.add(p), (shown = null), draw()) }))));
    });
    // (A global: where it is declared, a link to it in the source.)
    const decl = n.kind === "global" ? m.definition(n.what) : null;
    // (The member it hangs from, unless its name says it: prog(19)->real_cred.)
    const via = n.under && !n.what.endsWith(`->${n.under[1]}`) ? `  ← ${n.under[1]}` : "";
    return el("div", { className: `obj ${n.kind}`, dataset: { k } },
      // (As C declares it, the type first: one a pointer reaches, "*": struct file *filp in do_file_open.)
      el("div", { className: "title" }, el("span", { className: "dim", textContent: `${n.ty} ${n.kind === "object" ? "*" : ""}` }), el("b", { textContent: n.what }),
        el("span", { className: "dim", textContent: `  at 0x${n.addr.toString(16)}${via}` }),
        decl ? el("a", { className: "decl", textContent: `  ${decl.file}:${decl.line}`, title: "where it is declared", onclick: () => app.src.show(decl.file, decl.line) }) : null),
      el("table", { className: "objs" }, el("tbody", {}, ...rows)));
  }

  // The moment `cur` (a record): the runs of writes not begun by then
  // gray, the one it is in (from its first write to its last) marked.
  function mark(cur) {
    for (const r of body.querySelectorAll("tr[data-last]")) {
      const first = Number(r.dataset.first), last = Number(r.dataset.last);
      r.classList.toggle("later", cur != null && first > cur);
      r.classList.toggle("on", cur != null && first <= cur && cur <= last);
    }
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
