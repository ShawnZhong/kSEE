// The Objects: what the selected call (a syscall, or a call clicked in the
// timeline) changed in the kernel's data, as a diagram (Machine.objects):
// a box per object it wrote (named as the Stores name them: "vfs_read's
// file", "current->files"), its members written, from what to what (the
// write in effect at the cursor; later ones dim; each a link to that
// moment), and its pointers to the other objects as arrows, as memory was
// at the cursor. The Stores list the writes; this shows how the objects
// hang together (a kiocb's file, the file's inode, the inode's page). An
// object on the stack whose call has returned by the cursor is gone (its
// slot another's): shown as it was then.
import { $, el } from "./dom.js";
import { tree } from "./tree.js";

export function objects(app) {
  const { m } = app;
  const body = $("objects");
  let shown = null, last = null;
  // (A box opened: all of its object, as the Stack's values are, a tree.)
  const st = { open: new Set(), last: new Map(), now: new Map() };
  const { rows: fields } = tree(app, st, () => last && render(...last));

  // Draw for the selection at the cursor (only while the tab is shown:
  // again as the cursor moves, the pointers being memory's then).
  function draw() {
    if ($("right").dataset.tab !== "objects") return;
    const w = app.selection(), cur = app.time.cursor();
    const key = w ? `${w.key}:${cur}` : null;
    if (key === shown) return;
    shown = key;
    $("objects-note").textContent = w ? "" : "Select a call (click one in the program, or a syscall in the timeline): the objects it wrote, and how they point to each other.";
    if (!w) return body.replaceChildren();
    render(m.objects(w.a, w.b, cur ?? w.b), cur ?? Infinity);
  }

  function render(nodes, cur) {
    last = [nodes, cur];
    st.last = st.now;
    st.now = new Map();
    // (In the order the pointers lead: from each object nothing points to,
    // depth first; then any left.)
    const pointed = new Set(nodes.flatMap((n) => n.links.map((l) => l.to)));
    const order = [], seen = new Set();
    const visit = (k) => {
      if (seen.has(k)) return;
      seen.add(k);
      order.push(k);
      nodes[k].links.forEach((l) => visit(l.to));
    };
    nodes.forEach((_, k) => pointed.has(k) || visit(k));
    nodes.forEach((_, k) => visit(k));
    const boxes = order.map((k) => box(nodes[k], k, cur));
    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    svg.classList.add("arrows");
    body.replaceChildren(el("div", { className: "dim head" }, `${nodes.length} object${nodes.length === 1 ? "" : "s"} written`), ...boxes, svg);
    arrows(svg, nodes);
  }

  // An object's box: its name and type, its members written (the write in
  // effect at the cursor, else the first), its pointers to the others.
  function box(n, k, cur) {
    const byMember = new Map();
    for (const x of n.writes) (byMember.get(x.member) ?? byMember.set(x.member, []).get(x.member)).push(x);
    // (Not a whole aggregate's write, a memset: its members' say it.)
    const rows = [...byMember].filter(([, xs]) => xs.some((x) => x.before !== "{...}" || x.after !== "{...}")).map(([member, xs]) => {
      const done = xs.filter((x) => x.i <= cur), x = done.at(-1) ?? xs[0];
      return el("div", { className: `row${done.length ? "" : " later"}${x.before === x.after ? " same" : ""}` },
        el("a", { textContent: `${member || "(all of it)"}: ${x.before} → ${x.after}`, title: `go to this write${xs.length > 1 ? ` (${done.length} of ${xs.length} by the cursor)` : ""}`, onclick: () => app.time.go(x.i) }),
        xs.length > 1 ? el("span", { className: "dim", textContent: ` ×${xs.length}` }) : null);
    });
    const links = n.links.map((l) => el("div", { className: "row link", dataset: { to: l.to } },
      el("span", { textContent: `${l.member} →` })));
    // (An object on the stack whose call has returned: gone, its slot
    // another's; as it was when it went.)
    // (Its title opens it: all of it, as memory was at the cursor.)
    const path = `obj/${n.addr}/${n.id}`, open = st.open.has(path), all = [];
    if (open) fields(m.object_by_id(n.what, n.id, n.addr, 1)?.children ?? [], path, 0, all);
    const toggle = () => (open ? st.open.delete(path) : st.open.add(path), render(...last));
    return el("div", { className: `obj ${n.kind}${n.ended != null ? " ended" : ""}`, dataset: { k },
                       title: n.ended != null ? "on the stack of a call that has returned by the cursor: as it was then" : "" },
      el("div", { className: "title", onclick: toggle, title: open ? "fold it" : "all of it, as it was at the cursor" },
        el("span", { className: "tw", textContent: open ? "▾" : "▸" }), el("b", { textContent: n.what }),
        el("span", { className: "dim", textContent: ` ${n.ty}${n.ended != null ? " · gone" : ""}` })),
      ...rows, ...links, open ? el("table", { className: "objs" }, el("tbody", {}, ...all)) : null);
  }

  // The pointers, as arrows from a member's row to its object's title, in
  // the margin on the right (each further out, to be told apart).
  function arrows(svg, nodes) {
    const top = body.getBoundingClientRect(), at = (e) => e.getBoundingClientRect();
    svg.setAttribute("width", body.scrollWidth);
    svg.setAttribute("height", body.scrollHeight);
    const path = (d) => {
      const p = document.createElementNS("http://www.w3.org/2000/svg", "path");
      p.setAttribute("d", d);
      svg.append(p);
    };
    [...body.querySelectorAll(".row.link")].forEach((row, j) => {
      const to = body.querySelector(`.obj[data-k="${row.dataset.to}"] .title`);
      if (!to) return;
      const a = at(row), b = at(to), y = (r) => r.top - top.top + body.scrollTop + r.height / 2;
      const x0 = a.right - top.left, x1 = b.right - top.left, out = 12 + (j % 5) * 7;
      const [y0, y1] = [y(a), y(b)];
      path(`M${x0},${y0} C${x0 + out},${y0} ${x1 + out},${y1} ${x1 + 2},${y1}`);
      path(`M${x1 + 7},${y1 - 4} L${x1 + 2},${y1} L${x1 + 7},${y1 + 4}`);
    });
  }

  const mark = () => draw();
  return { draw, mark };
}
