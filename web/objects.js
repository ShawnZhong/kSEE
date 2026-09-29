// The Objects: what the call in focus did to the kernel's data
// (Machine.objects): the syscall selected, or a call in it clicked in the
// timeline (or an example's link), or, while the pointer is over one in
// the timeline, that one (a preview, as the Stack and the source show
// that moment): a head saying it in a line ("write
// changed 2 objects: file, inode"), then a box per object it changed, the
// kernel's data, not a stack's (a call's locals: the Stack), first those
// it left changed, then, folded, those it left as they were (a lock taken
// and released, a count taken and dropped) (declared as the code names them: "struct file *file in vfs_read",
// "struct files_struct *prog(19)->files"; and by
// its address, which it is; a global, where it is declared), each
// member it changed, a line a function that changed it in turn, from
// what to what ("f_mode  0 → 0x4000001  init_file", then "0x4000001 →
// 0x40a801d  do_dentry_open"; one left as it was, a count one took and
// another dropped, dim; click: that write). As a tree: each box under the first that
// points to it, by that member ("← f_inode"); in a box, the members it
// left changed first. The whole call, at every moment; at the moment shown
// (the cursor's, or the pointer's), the writes not made yet gray, the run
// of writes it is in marked.
import { $, el, taskName } from "./dom.js";

export function objects(app) {
  const { m } = app;
  const body = $("objects");
  let shown = null, selected = null;
  // (The call under the pointer in the timeline, by its first record,
  // once the pointer rests on it; null: none.)
  let over = null, wait = 0;
  // The call shown: the one under the pointer, else the one in focus
  // (keyed, as app.focus is: see there).
  function shownCall() {
    if (over == null) return app.focus();
    const b = m.call_end(over) ?? m.records() - 1, s = app.syscalls().find((c) => c.first === over);
    const title = s ? `${s.name ?? s.nr} by ${taskName(s.task)}` : m.at(over)?.func ?? "a call";
    return { a: over, b, title, over: true, key: `over:${over}:${b}:${m.record_id(m.records() - 1)}` };
  }
  // The pointer over call `first` in the timeline (null: over none, or
  // gone): shown once it rests there (asking is not free, for a big call);
  // back at once.
  function preview(first) {
    clearTimeout(wait);
    if (first === over) return;
    if (first == null) return ((over = null), draw());
    wait = setTimeout(() => ((over = first), draw()), 120);
  }
  // (The members whose runs are shown in full, past six, by box and
  // member: "3/0"; for the selection.)
  const open = new Set();

  // Draw for the call in focus (only while the tab is shown; again for
  // another call or recording).
  function draw() {
    if ($("right").dataset.tab !== "objects") return;
    const w = shownCall(), key = w?.key ?? null;
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

  // (Under the pointer: says so; a call in the syscall: a way back to
  // the whole syscall.)
  function whole(w) {
    if (w.over) return [el("span", { className: "dim", textContent: "  · under the pointer (click it to keep it)" })];
    const h = app.selection();
    return h && (h.a !== w.a || h.b !== w.b) ? ["  · ", el("a", { textContent: `the whole ${h.title}`, onclick: () => app.time.unfocus() })] : [];
  }

  // (A box's name, short: its expression, not where it is named.)
  const short = (n) => n.what.split(" in ")[0];

  function render(nodes, w) {
    const title = w.title;
    if (!nodes.length) return [el("div", { className: "dim head" }, `${title} changed nothing of the kernel's data`, ...whole(w))];
    const changed = nodes.filter((n) => n.group === "changed"), same = nodes.filter((n) => n.group === "same");
    const count = (list) => `${list.length} object${list.length === 1 ? "" : "s"}`;
    // (The head: what it changed, in a line; what it left as it was.)
    const head = [changed.length ? `${title} changed ${count(changed)}: ${changed.map(short).join(", ")}` : `${title} left the kernel's data as it was`,
                  same.length && changed.length ? `; left ${count(same)} as it was` : ""].join("");
    const out = [el("div", { className: "head" }, head, ...whole(w))];
    // (Each group as a tree: each box under the one that points to it
    // (Node.under, of its group), from the ones nothing points to.)
    const tree = (group) => {
      const order = [], visit = (k, depth) => (order.push([k, depth]), nodes.forEach((n, c) => n.under?.[0] === k && visit(c, depth + 1)));
      nodes.forEach((n, k) => n.group === group && n.under == null && visit(k, 0));
      // (One under another: indented, a guide line down its left.)
      return order.map(([k, depth]) => {
        const b = box(nodes[k], nodes, k);
        if (depth) (b.classList.add("under"), (b.style.marginLeft = `${Math.min(depth, 6) * 18}px`));
        return b;
      });
    };
    out.push(...tree("changed"));
    // (Those left as they were: folded, a line naming them; on asking, their boxes.)
    if (same.length) {
      const unfolded = open.has("same");
      out.push(el("div", { className: "dim head fold" }, el("a", {
        textContent: `${unfolded ? "▾" : "▸"} left as it was: ${same.map(short).join(", ")}`,
        title: "changed, then changed back by the end (a lock taken and released, a count taken and dropped)",
        onclick: () => (unfolded ? open.delete("same") : open.add("same"), (shown = null), draw()) })));
      if (unfolded) out.push(...tree("same"));
    }
    return out;
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

  // (The moment shown: the pointer's, or the cursor's.)
  return { draw, preview, mark: (i) => $("right").dataset.tab === "objects" && mark(i ?? app.time.cursor()) };
}
