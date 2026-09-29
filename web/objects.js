// The Objects: what the syscall selected did to the kernel's data
// (Machine.objects), all of it, always: a head saying it in a line ("write
// changed 2 objects: file, inode"), then a box per object it changed, the
// kernel's data, not a stack's (a call's locals: the Stack), first those
// it left changed, then those it left as they were (a lock taken and
// released, a count taken and dropped) (declared as the code names them:
// "struct file *file in vfs_read", "struct files_struct *prog(19)->files";
// and by its address, which it is; a global, where it is declared), each
// member it changed, a line a function that changed it in turn, from what
// to what ("f_mode  0 → 0x4000001  init_file", then "0x4000001 →
// 0x40a801d  do_dentry_open"; one left as it was, a count one took and
// another dropped, dim; click: that write). As a tree: each box under the
// first that points to it, by that member ("← f_inode"); in a box, the
// members it left changed first. The timeline only colors it: the call
// under the pointer, or else the one clicked (or an example's), its
// writes in color, the others gray; with neither, the writes not made by
// the cursor gray. The run of writes the cursor is in, marked.
import { $, el } from "./dom.js";

export function objects(app) {
  const { m } = app;
  const body = $("objects");
  let shown = null;
  // (The call under the pointer in the timeline, by its first record;
  // null: none.)
  let over = null;

  // Draw for the syscall selected (only while the tab is shown; again for
  // another one, or recording).
  function draw() {
    if ($("right").dataset.tab !== "objects") return;
    const w = app.selection(), key = w?.key ?? null;
    if (key !== shown) {
      shown = key;
      $("objects-note").textContent = w ? "" : "Select a syscall (click one in the program, or in the timeline): what it did to the kernel's data.";
      const top = body.scrollTop;
      body.replaceChildren(...(w ? render(m.objects(w.a, w.b), w.title) : []));
      body.scrollTop = top;
    }
    mark(app.time.cursor());
  }

  // The call whose writes are in color: the one under the pointer, else
  // the one in focus (clicked, or an example's), if not the syscall
  // itself: { a, b, title }; null: none.
  function lit() {
    const h = app.selection();
    if (over != null) return { a: over, b: m.call_end(over) ?? m.records() - 1, title: m.at(over)?.func ?? "a call" };
    const f = app.focus();
    return f && h && (f.a !== h.a || f.b !== h.b) ? f : null;
  }
  // The pointer over call `first` in the timeline (null: over none, or gone).
  function preview(first) {
    if (first === over) return;
    over = first;
    mark(app.time.cursor());
  }

  // (A box's name, short: its expression, not where it is named.)
  const short = (n) => n.what.split(" in ")[0];

  function render(nodes, title) {
    if (!nodes.length) return [el("div", { className: "dim head" }, `${title} changed nothing of the kernel's data`)];
    const changed = nodes.filter((n) => n.group === "changed"), same = nodes.filter((n) => n.group === "same");
    const count = (list) => `${list.length} object${list.length === 1 ? "" : "s"}`;
    // (The head: what it changed, in a line; what it left as it was.)
    const head = [changed.length ? `${title} changed ${count(changed)}: ${changed.map(short).join(", ")}` : `${title} left the kernel's data as it was`,
                  same.length && changed.length ? `; left ${count(same)} as it was` : ""].join("");
    const out = [el("div", { className: "head" }, head, el("span", { className: "dim lit" }))];
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
    // (Then those left as they were, under a head of their own.)
    if (same.length) {
      out.push(el("div", { className: "dim head", title: "changed, then changed back by the end (a lock taken and released, a count taken and dropped)" },
        `Left as it was, changed and changed back: ${same.map(short).join(", ")}`));
      out.push(...tree("same"));
    }
    return out;
  }

  // An object's box: its name, its type, the member it hangs from; each
  // member changed, a line a function that changed it in turn, from what
  // to what (a pointer to another box, at the end: that box, a link to
  // it). One left as it was, dim.
  function box(n, nodes, k) {
    const rows = [];
    n.changes.forEach((d) => {
      const to = d.to != null ? nodes[d.to] : null, runs = d.runs;
      runs.forEach((r, q) => rows.push(el("tr", { className: d.before === d.after ? "same" : "", dataset: { first: r.first, last: r.last }, onclick: () => app.time.go(r.last),
          title: `${r.writes === 1 ? "written once" : `written ${r.writes} times`}${r.at ? ` in ${r.at.func} (${r.at.file}:${r.at.line})` : ""}: click to go there` },
        el("td", { className: "fname" }, q ? "" : d.member || "(itself)"),
        el("td", { className: "fval" }, el("span", { className: "dim" }, `${r.before} → `),
          q === runs.length - 1 && to ? el("a", { textContent: to.what, onclick: (e) => (e.stopPropagation(), flash(d.to)) }) : r.after),
        el("td", { className: "ftype" }, r.at?.func ?? ""))));
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

  // The colors: the call lit (see lit), its runs of writes (any write in
  // its records) in color, the others gray; none, the runs not begun by
  // the cursor `cur` gray. The run the cursor is in, marked; the head says
  // whose writes are in color.
  function mark(cur) {
    const l = lit();
    for (const r of body.querySelectorAll("tr[data-last]")) {
      const first = Number(r.dataset.first), last = Number(r.dataset.last);
      r.classList.toggle("later", l ? last < l.a || first > l.b : cur != null && first > cur);
      r.classList.toggle("on", cur != null && first <= cur && cur <= last);
    }
    const h = body.querySelector(".head .lit");
    if (h) h.textContent = l ? `  · in color: what ${l.title} wrote` : "";
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

  // (The cursor's moment: the pointer colors by its call, not its moment.)
  return { draw, preview, mark: () => $("right").dataset.tab === "objects" && mark(app.time.cursor()) };
}
