// The Objects: what the selected call (a syscall, or a call clicked in the
// timeline) changed in the kernel's data, at the cursor (Machine.objects):
// a box per object it wrote (named as the Stores name them: "vfs_read's
// file", "current->files"), each member it writes as it is at the cursor
// ("f_pos = 18", and what it was before the call: "was 0"; one the call
// writes only later, dim, with what it will be), and its pointers to the
// other objects, each a link to it ("f_inode → atime_needs_update's
// inode": a kiocb's file, the file's inode, the inode's page). The member
// the cursor last wrote is marked, and in view: stepping shows each change
// as it is made. A member's writes on asking (each a link to its moment);
// all of its object's fields on asking, as the Stack's values are. An object
// on the stack is there while its call runs, as on the Stack; the ones
// gone (or not yet made), and the ones the call writes only after the
// cursor, on asking: what it changed so far first.
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
    body.querySelector(".row.on")?.scrollIntoView({ block: "nearest" });
  }

  // An object's box: its name and type; each member it writes, as it is at
  // the cursor; its pointers to the others.
  function box(n, nodes, k, cur, now, alive, shownAt) {
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
    // (Its pointers to the others: each a link to it.)
    const links = n.links.filter((l) => shownAt(l.to)).map((l) => el("div", { className: "row link" },
      el("span", { textContent: `${l.member} → ` }),
      el("a", { textContent: nodes[l.to].what, title: `${nodes[l.to].ty} at 0x${l.value.toString(16)}: go to it`, onclick: () => flash(l.to) })));
    // (All of it, as memory was at the cursor, on asking: its title, or
    // the line at its end.)
    const path = `obj/${n.addr}/${n.id}`, open = st.open.has(path), all = [];
    const children = open ? m.object_by_id(n.what, n.id, n.addr, 1)?.children ?? [] : [];
    if (open) fields(children, path, 0, all);
    const toggle = () => (open ? st.open.delete(path) : st.open.add(path), redraw());
    const more = el("div", { className: "row more" }, el("a", { textContent: open ? `▾ only what it wrote` : "▸ all its fields", onclick: toggle }));
    const note = alive ? "" : n.life && cur < n.life[0] ? " · not made yet" : " · gone";
    return el("div", { className: `obj ${n.kind}${alive ? "" : " ended"}`, dataset: { k },
                       title: alive ? "" : "on the stack of a call not running at the cursor: as it was while it did" },
      el("div", { className: "title", onclick: toggle, title: open ? "fold it" : "all of it, as it was at the cursor" },
        el("span", { className: "tw", textContent: open ? "▾" : "▸" }), el("b", { textContent: n.what }),
        el("span", { className: "dim", textContent: ` ${n.ty}${note}` })),
      ...rows, ...links, open ? el("table", { className: "objs" }, el("tbody", {}, ...all)) : null, more);
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
