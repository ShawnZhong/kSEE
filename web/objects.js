// The Objects: what the selected call (a syscall, or a call double-clicked
// in the timeline: the zoom) changed in the kernel's data (Machine.objects),
// at the moment the page shows: a box per object it writes (named as the
// code names them: "vfs_read's file", "current->files"), in the order of
// the calls that name them, the outer first, each in its place at every
// moment (one not written yet, dim; one on the stack of a call not running
// then, dashed). Each member it writes as it is then, in the Stack's value
// rows (tree.js), the one last written marked; its pointers to the other
// objects, each a link to it ("f_inode → atime_needs_update's inode"); all
// of its fields on asking.
import { $, el } from "./dom.js";
import { tree } from "./tree.js";

export function objects(app) {
  const { m } = app;
  const body = $("objects");
  let shown = null, selected = null, last = null;
  // (The values' state, as the Stack's: tree.js; a box's path in `open`:
  // all of its fields.)
  const st = {}, seen = new Set();
  const redraw = () => last && render(...last);
  const { rows: fields, moved } = tree(app, st, redraw);

  // Draw for the selection at record `cur`: the moment the page shows (the
  // cursor, or the pointer's over the timeline; only while the tab is
  // shown: again as it moves).
  function draw(cur = app.time.cursor()) {
    if ($("right").dataset.tab !== "objects") return;
    const w = app.selection();
    const key = w ? `${w.key}:${cur}` : null;
    if (key === shown) return;
    // (Another selection, or recording: its boxes open to what it wrote again.)
    if (w?.key !== selected) (seen.clear(), (selected = w?.key));
    shown = key;
    $("objects-note").textContent = w ? "" : "Select a call (click one in the program, or a syscall in the timeline): the objects it wrote, and how they point to each other.";
    if (!w) return body.replaceChildren();
    moved(); // (another moment: what changed since, marked; not on a redraw)
    render(m.objects(w.a, w.b, cur ?? w.b), cur ?? w.b);
  }

  function render(nodes, cur) {
    last = [nodes, cur];
    // (All of them, in Machine.objects' order (the calls that name them,
    // outer first), each in its place at every moment: one on the stack of
    // a call not running then, dashed; one not written yet, dim.)
    const live = (n) => !n.life || (n.life[0] <= cur && cur <= n.life[1]);
    const soFar = nodes.filter((n) => live(n) && n.writes[0].i <= cur).length;
    // (The write the cursor is at, or last passed: marked.)
    const now = nodes.flatMap((n) => n.writes).filter((x) => x.i <= cur).reduce((a, x) => (!a || x.i > a.i ? x : a), null);
    body.replaceChildren(el("div", { className: "dim head" }, `${nodes.length} object${nodes.length === 1 ? "" : "s"} written, ${soFar} so far`),
      ...nodes.map((n, k) => box(n, nodes, k, cur, now, live(n))));
    body.querySelector("tr.on")?.scrollIntoView({ block: "nearest" });
  }

  // An object's box: its name and type; the members it writes (or, on
  // asking, all of them), as they are at the cursor; its pointers to the
  // others.
  function box(n, nodes, k, cur, now, alive) {
    const path = `obj/${n.addr}/${n.id}`, all = st.open.has(path);
    const obj = m.object_by_id(n.what, n.id, n.addr, 1);
    // (Each member it writes, by the name its object has: a nested one's
    // outermost, "f_path" for "f_path.dentry"; a scalar's own value, the
    // object itself.)
    const top = (member) => member.match(/^\[\d+\]|^[^.[]*/)[0];
    const byMember = new Map();
    for (const x of n.writes) (byMember.get(top(x.member)) ?? byMember.set(top(x.member), []).get(top(x.member))).push(x);
    const members = obj?.children ? obj.children.filter((c) => all || byMember.has(c.name)) : obj ? [obj] : [];
    // (The way to each member it writes inside another, open the first time
    // the box is drawn: "_refcount.counter" opens _refcount.)
    if (!seen.has(path)) {
      seen.add(path);
      for (const x of n.writes) {
        const parts = x.member.split(/\.|(?=\[)/).filter(Boolean);
        for (let j = 1; j < parts.length; j++) st.open.add(`${path}.${parts.slice(0, j).join(".")}`);
      }
    }
    const life = n.life ? { from: n.life[0], to: n.life[1], what: "while its frame lives" } : null, rows = [];
    for (const c of members) {
      const from = rows.length, xs = byMember.get(obj?.children ? c.name : "") ?? [];
      fields([c], path, 0, rows, life);
      // (Written only after the cursor: dim; the member last written: marked.)
      if (xs.length && xs.every((x) => x.i > cur)) rows[from].classList.add("later");
      if (now && xs.includes(now)) rows[from].classList.add("on");
    }
    // (Its pointers to the others: each a link to it.)
    const links = n.links.map((l) => el("div", { className: "row link" },
      el("span", { textContent: `${l.member} → ` }),
      el("a", { textContent: nodes[l.to].what, title: `${nodes[l.to].ty} at 0x${l.value.toString(16)}: go to it`, onclick: () => flash(l.to) })));
    const toggle = () => (all ? st.open.delete(path) : st.open.add(path), redraw());
    const more = obj?.children ? el("div", { className: "row more" }, el("a", { textContent: all ? "▾ only what it writes" : `▸ all ${obj.children.length} fields`, onclick: toggle })) : null;
    const note = alive ? "" : n.life && cur < n.life[0] ? " · not made yet" : " · gone";
    const later = n.writes[0].i > cur ? " later" : "";
    return el("div", { className: `obj ${n.kind}${alive ? "" : " ended"}${later}`, dataset: { k },
                       title: alive ? "" : "on the stack of a call not running at the cursor: its memory is not its own then" },
      el("div", { className: "title" }, el("b", { textContent: n.what }), el("span", { className: "dim", textContent: ` ${n.ty}${note}` })),
      el("table", { className: "objs" }, el("tbody", {}, ...rows)), ...links, more);
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

  return { draw: () => draw(), mark: (i) => draw(i ?? app.time.cursor()) };
}
