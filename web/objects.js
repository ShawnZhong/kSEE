// The Objects: what the selected call (a syscall, or a call clicked in the
// timeline) changed in the kernel's data, at the cursor (Machine.objects):
// a box per object it wrote (named as the Stores name them: "vfs_read's
// file", "current->files"), each member it writes as it is at the cursor
// ("f_pos = 18", and what it was before the call: "was 0"; one the call
// writes only later, dim, with what it will be), and its pointers to the
// other objects as arrows (a kiocb's file, the file's inode, the inode's
// page). The member the cursor last wrote is marked, and in view: stepping
// shows each change as it is made. A member's writes on asking (each a
// link to its moment); a box's title opens all of its object. An object
// on the stack is there while its call runs, as on the Stack; the ones
// gone (or not yet made) on asking.
import { $, el } from "./dom.js";
import { tree } from "./tree.js";

export function objects(app) {
  const { m } = app;
  const body = $("objects");
  let shown = null, last = null, gone = false;
  // (Members whose writes are shown; a box opened: all of its object, as
  // the Stack's values are, a tree.)
  const writes = new Set();
  const st = { open: new Set(), last: new Map(), now: new Map() };
  const redraw = () => last && render(...last);
  const { rows: fields } = tree(app, st, redraw);

  // Draw for the selection at the cursor (only while the tab is shown:
  // again as the cursor moves).
  function draw() {
    if ($("right").dataset.tab !== "objects") return;
    const w = app.selection(), cur = app.time.cursor();
    const key = w ? `${w.key}:${cur}` : null;
    if (key === shown) return;
    shown = key;
    $("objects-note").textContent = w ? "" : "Select a call (click one in the program, or a syscall in the timeline): the objects it wrote, and how they point to each other.";
    if (!w) return body.replaceChildren();
    render(m.objects(w.a, w.b, cur ?? w.b), cur ?? w.b);
  }

  function render(nodes, cur) {
    last = [nodes, cur];
    st.last = st.now;
    st.now = new Map();
    // (On the stack: there while its call runs.)
    const live = (n) => !n.life || (n.life[0] <= cur && cur <= n.life[1]);
    const away = nodes.filter((n) => !live(n)).length;
    const shownAt = (k) => gone || live(nodes[k]);
    // (In the order the pointers lead: from each object nothing points to,
    // depth first; then any left.)
    const pointed = new Set(nodes.flatMap((n) => n.links.map((l) => l.to)));
    const order = [], seen = new Set();
    const visit = (k) => {
      if (seen.has(k) || !shownAt(k)) return;
      seen.add(k);
      order.push(k);
      nodes[k].links.forEach((l) => visit(l.to));
    };
    nodes.forEach((_, k) => pointed.has(k) || visit(k));
    nodes.forEach((_, k) => visit(k));
    // (The write the cursor is at, or last passed: marked.)
    const now = nodes.flatMap((n) => n.writes).filter((x) => x.i <= cur).reduce((a, x) => (!a || x.i > a.i ? x : a), null);
    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    svg.classList.add("arrows");
    body.replaceChildren(el("div", { className: "dim head" }, `${order.length} object${order.length === 1 ? "" : "s"} written`,
      away ? el("a", { textContent: gone ? ` · hide the ${away} not alive at the cursor` : ` · ${away} more on the stack, not alive at the cursor`,
                       title: "objects on the stack of a call not running at the cursor (returned, or not called yet)", onclick: () => ((gone = !gone), redraw()) }) : ""),
      ...order.map((k) => box(nodes[k], k, cur, now, live(nodes[k]), shownAt)), svg);
    arrows(svg);
    body.querySelector(".row.on")?.scrollIntoView({ block: "nearest" });
  }

  // An object's box: its name and type; each member it writes, as it is at
  // the cursor; its pointers to the others.
  function box(n, k, cur, now, alive, shownAt) {
    const byMember = new Map();
    for (const x of n.writes) (byMember.get(x.member) ?? byMember.set(x.member, []).get(x.member)).push(x);
    // (Not a whole aggregate's write, a memset: its members' say it.)
    const members = [...byMember].filter(([, xs]) => xs.some((x) => x.before !== "{...}" || x.after !== "{...}"));
    const rows = members.flatMap(([member, xs]) => {
      const done = xs.filter((x) => x.i <= cur), key = `${n.addr}:${n.id}:${member}`, open = writes.has(key);
      // (A scalar's own value: by its name, "pos".)
      const name = member || n.what.split(/\W/).pop();
      // (Written by the cursor: its value then, and what it was before;
      // not yet: as it is, and what it will be.)
      const text = done.length
        ? [`${name} = ${done.at(-1).after}`, xs[0].before !== done.at(-1).after ? `was ${xs[0].before}` : ""]
        : [`${name} = ${xs[0].before}`, `→ ${xs.at(-1).after} later`];
      const row = el("div", { className: `row${done.length ? "" : " later"}${done.includes(now) ? " on" : ""}` },
        el("a", { textContent: text[0], title: "go to its last write by the cursor (its first, if none yet)", onclick: () => app.time.go((done.at(-1) ?? xs[0]).i) }),
        text[1] ? el("span", { className: "dim", textContent: `  ${text[1]}` }) : null,
        el("a", { className: "count", textContent: `  ${xs.length} write${xs.length === 1 ? "" : "s"}`, title: open ? "fold its writes" : "its writes, each a link to its moment",
                  onclick: () => (open ? writes.delete(key) : writes.add(key), redraw()) }));
      if (!open) return [row];
      return [row, el("ol", { className: "writes" }, ...xs.map((x) => el("li", { className: `${x.i > cur ? "later" : ""}${x === done.at(-1) ? " on" : ""}` },
        el("a", { textContent: `${x.before} → ${x.after}`, title: "go to this write", onclick: () => app.time.go(x.i) }),
        el("span", { className: "dim", textContent: ` in ${x.at?.func ?? "?"}` }))))];
    });
    const links = n.links.filter((l) => shownAt(l.to)).map((l) => el("div", { className: "row link", dataset: { to: l.to } }, el("span", { textContent: `${l.member} →` })));
    // (Its title opens it: all of it, as memory was at the cursor.)
    const path = `obj/${n.addr}/${n.id}`, open = st.open.has(path), all = [];
    if (open) fields(m.object_by_id(n.what, n.id, n.addr, 1)?.children ?? [], path, 0, all);
    const toggle = () => (open ? st.open.delete(path) : st.open.add(path), redraw());
    const note = alive ? "" : n.life && cur < n.life[0] ? " · not made yet" : " · gone";
    return el("div", { className: `obj ${n.kind}${alive ? "" : " ended"}`, dataset: { k },
                       title: alive ? "" : "on the stack of a call not running at the cursor: as it was while it did" },
      el("div", { className: "title", onclick: toggle, title: open ? "fold it" : "all of it, as it was at the cursor" },
        el("span", { className: "tw", textContent: open ? "▾" : "▸" }), el("b", { textContent: n.what }),
        el("span", { className: "dim", textContent: ` ${n.ty}${note}` })),
      ...rows, ...links, open ? el("table", { className: "objs" }, el("tbody", {}, ...all)) : null);
  }

  // The pointers, as arrows from a member's row to its object's title, in
  // the margin on the right (each further out, to be told apart).
  function arrows(svg) {
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
