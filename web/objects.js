// The Objects: what the selected call (a syscall, or a call clicked in the
// timeline) changed in the kernel's data, at the cursor (Machine.objects):
// a box per object it wrote (named as the Stores name them: "vfs_read's
// file", "current->files"), each member it writes as it is at the cursor,
// in the Stack's value rows (tree.js: a pointer opens into what it points
// to, a value's writes on asking, a value changed since the
// cursor last moved is marked); one the call writes only after the
// cursor, dim; the one the cursor last wrote, marked. Its pointers to the
// other objects, each a link to it ("f_inode → atime_needs_update's
// inode"). All of its fields on asking. An object on the stack is there
// while its call runs, as on the Stack; the ones gone (or not yet made),
// and the ones the call writes only after the cursor, on asking.
import { $, el } from "./dom.js";
import { tree } from "./tree.js";

export function objects(app) {
  const { m } = app;
  const body = $("objects");
  let shown = null, last = null, gone = false;
  // (The values' state, as the Stack's: tree.js; a box's path in `open`:
  // all of its fields.)
  const st = { open: new Set(), last: new Map(), now: new Map() }, seen = new Set();
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
    // (On the stack: there while its call runs; and written by the
    // cursor. The others on asking.)
    const live = (n) => !n.life || (n.life[0] <= cur && cur <= n.life[1]);
    const soFar = (n) => live(n) && n.writes[0].i <= cur;
    const away = nodes.filter((n) => !soFar(n)).length;
    const shownAt = (k) => gone || soFar(nodes[k]);
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
    body.replaceChildren(el("div", { className: "dim head" }, `${order.length} object${order.length === 1 ? "" : "s"} written${gone ? "" : " so far"}`,
      away ? el("a", { textContent: gone ? ` · only those written so far` : ` · ${away} more, written later or not alive at the cursor`,
                       title: "objects the call writes after the cursor, and objects on the stack of a call not running at the cursor (returned, or not called yet)",
                       onclick: () => ((gone = !gone), redraw()) }) : ""),
      ...order.map((k) => box(nodes[k], nodes, k, cur, now, live(nodes[k]), shownAt)));
    body.querySelector("tr.on")?.scrollIntoView({ block: "nearest" });
  }

  // An object's box: its name and type; the members it writes (or, on
  // asking, all of them), as they are at the cursor; its pointers to the
  // others.
  function box(n, nodes, k, cur, now, alive, shownAt) {
    const path = `obj/${n.addr}/${n.id}`, all = st.open.has(path);
    const obj = m.object_by_id(n.what, n.id, n.addr, 1);
    // (Each member it writes, by the name its object has: a nested one's
    // outermost, "f_path" for "f_path.dentry"; a scalar's own value, the
    // object itself.)
    const top = (member) => member.match(/^[^.[]*|^\[\d+\]/)[0];
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
    const links = n.links.filter((l) => shownAt(l.to)).map((l) => el("div", { className: "row link" },
      el("span", { textContent: `${l.member} → ` }),
      el("a", { textContent: nodes[l.to].what, title: `${nodes[l.to].ty} at 0x${l.value.toString(16)}: go to it`, onclick: () => flash(l.to) })));
    const toggle = () => (all ? st.open.delete(path) : st.open.add(path), redraw());
    const more = obj?.children ? el("div", { className: "row more" }, el("a", { textContent: all ? "▾ only what it writes" : `▸ all ${obj.children.length} fields`, onclick: toggle })) : null;
    const note = alive ? "" : n.life && cur < n.life[0] ? " · not made yet" : " · gone";
    return el("div", { className: `obj ${n.kind}${alive ? "" : " ended"}`, dataset: { k },
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

  const mark = () => draw();
  return { draw, mark };
}
