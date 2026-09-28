// The Objects: what the selected call (a syscall, or a call double-clicked
// in the timeline: the zoom) did to the kernel's data (Machine.objects):
// a box per object it changed that is still there when it returns (named
// as the code names them: "vfs_read's file", "prog(19)->files"), each
// member it changed from what it was to what it is as the call ends
// (click: the write that left it so); the members it wrote back as they
// were, in a line (and the objects it left as they were, in one at the
// end); its pointers to the others, each a link to it
// ("f_inode → filemap_read's inode"). As a tree: each box under the first
// that points to it. The same at every moment: the moments are the Stack's.
import { $, el } from "./dom.js";

export function objects(app) {
  const { m } = app;
  const body = $("objects");
  let shown = null;

  // Draw for the selection (only while the tab is shown; again for another
  // selection or recording).
  function draw() {
    if ($("right").dataset.tab !== "objects") return;
    const w = app.selection();
    const n = m.records(), key = w ? `${w.key}:${n ? m.record_id(n - 1) : ""}` : null;
    if (key !== shown) {
      shown = key;
      $("objects-note").textContent = w ? "" : "Select a call (click one in the program, or a syscall in the timeline): what it did to the kernel's data.";
      body.replaceChildren(...(w ? render(m.objects(w.a, w.b)).filter(Boolean) : []));
    }
    mark(app.time.cursor());
  }

  function render(nodes) {
    // (The ones it only wrote back as they were (a lock taken and let go):
    // a line at the end, not boxes.)
    const same = nodes.filter((n) => !n.changes.length), changed = nodes.length - same.length;
    if (!changed) return [el("div", { className: "dim head" }, "It changed nothing that is still there when it returns (its own locals: the Stack).")];
    // (As a tree: each box under the one that points to it (Node.under),
    // from the ones nothing changed points to; siblings in order.)
    const order = [], up = (n) => (n.under != null && nodes[n.under].changes.length ? n.under : null);
    const visit = (k, depth) => (order.push([k, depth]), nodes.forEach((n, c) => n.changes.length && up(n) === k && visit(c, depth + 1)));
    nodes.forEach((n, k) => n.changes.length && up(n) == null && visit(k, 0));
    return [el("div", { className: "dim head" }, `${changed} object${changed === 1 ? "" : "s"} changed`),
      ...order.map(([k, depth]) => Object.assign(box(nodes[k], nodes, k), { style: `margin-left: ${Math.min(depth, 6) * 14}px` })),
      same.length ? el("div", { className: "dim head" }, `Written, but left as they were: ${same.map((n) => n.what).join(", ")}`) : null];
  }

  // An object's box: its name and type; each member changed, before → after;
  // the ones written back; its pointers to the others.
  function box(n, nodes, k) {
    const rows = n.changes.map((d) => el("tr", { dataset: { last: d.last }, onclick: () => app.time.go(d.last),
        title: `${d.writes === 1 ? "written once" : `written ${d.writes} times`}${d.at ? `, last in ${d.at.func} (${d.at.file}:${d.at.line})` : ""}: click to go there` },
      el("td", { className: "fname" }, d.member || "(itself)"),
      el("td", { className: "fval" }, el("span", { className: "dim" }, d.before), " → ", d.after)));
    const back = n.back.length ? el("div", { className: "row dim" }, `written back as they were: ${n.back.map((b) => b || "itself").join(", ")}`) : null;
    const links = n.links.map((l) => el("div", { className: "row link" },
      el("span", { textContent: `${l.member} → ` }),
      nodes[l.to].changes.length ? el("a", { textContent: nodes[l.to].what, title: `${nodes[l.to].ty} at 0x${l.value.toString(16)}: go to it`, onclick: () => flash(l.to) })
        : el("span", { textContent: nodes[l.to].what })));
    return el("div", { className: `obj ${n.kind}`, dataset: { k } },
      el("div", { className: "title" }, el("b", { textContent: n.what }), el("span", { className: "dim", textContent: ` ${n.ty}` })),
      el("table", { className: "objs" }, el("tbody", {}, ...rows)), back, ...links);
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
