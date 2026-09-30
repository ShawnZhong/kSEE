// The Objects: what the syscall selected did to the kernel's data
// (Machine.objects), all of it, always: a head saying it in a line ("write
// changed 2 objects: file, inode"), then a box per object it changed, the
// kernel's data, not a stack's (a call's locals: the Stack), first those
// it left changed (its own task's, then each other task's that ran
// meanwhile, a head each: "By jbd2/loop0-8(16), while it ran"), then
// those it left as they were (a lock taken and
// released, a count taken and dropped) (each by what it is, its type's
// name, "file", then as the code names it, "filp in do_file_open"; the
// task and a global by their names; and by its address, which it is; a
// global, where it is declared), each
// member it changed, a line a function that changed it in turn, from what
// to what ("f_mode  0 → 0x4000001  init_file", then "0x4000001 →
// 0x40a801d  do_dentry_open"; one left as it was, a count one took and
// another dropped, dim; click: that write). As a tree: each box under the
// first that points to it, by that member ("← f_inode"); in a box, the
// members it left changed first. The moment shown (the pointer's over
// the timeline, else the cursor's) only colors it: the writes not made by
// then gray, the run of writes it is in marked.
import { $, el, taskName } from "./dom.js";

export function objects(app) {
  const { m } = app;
  const body = $("objects");
  let shown = null;

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

  // (How the code names a box: "filp in do_file_open"; and a box, as a
  // link to it says it: "file (filp in do_file_open)", "prog(19)".)
  const named = (n) => (n.within ? `${n.what} in ${n.within}` : n.what);
  const called = (n) => (n.kind === "object" ? `${n.label} (${named(n)})` : n.label);

  function render(nodes, title) {
    if (!nodes.length) return [el("div", { className: "dim head" }, `${title} changed nothing of the kernel's data`)];
    const changed = nodes.filter((n) => n.group === "changed"), same = nodes.filter((n) => n.group === "same");
    const count = (list) => `${list.length} object${list.length === 1 ? "" : "s"}`;
    // (Their names, once each: "folio ×23".)
    const names = (list) => [...Map.groupBy(list, (n) => n.label)].map(([s, of]) => (of.length > 1 ? `${s} ×${of.length}` : s)).join(", ");
    // (Its own, then each other task's that first wrote them while it ran,
    // by task: Node.by.)
    const own = changed.filter((n) => !n.by), others = [...Map.groupBy(changed.filter((n) => n.by), (n) => taskName(n.by))];
    // (The head: what it changed, in a line; what the others did; what it
    // left as it was.)
    const head = [own.length ? `${title} changed ${count(own)}: ${names(own)}` : `${title} itself changed nothing of the kernel's data`,
                  others.length ? `; while it ran, ${others.length === 1 ? "another task" : `${others.length} other tasks`} changed ${count(others.flatMap(([, l]) => l))} more (${others.map(([t, l]) => `${t}: ${l.length}`).join(", ")})` : "",
                  same.length && changed.length ? `; left ${count(same)} as it was` : ""].join("");
    const out = [el("div", { className: "head" }, head)];
    // (Each group as a tree, in order (each box followed by those under
    // it: Node.under); one under another indented, a guide line down its
    // left.)
    const depth = [];
    nodes.forEach((n, k) => (depth[k] = n.under != null ? depth[n.under] + 1 : 0));
    const tree = (list) => nodes.flatMap((n, k) => {
      if (!list.includes(n)) return [];
      const b = box(n, nodes, k);
      if (depth[k]) (b.classList.add("under"), (b.style.marginLeft = `${Math.min(depth[k], 6) * 18}px`));
      return [b];
    });
    out.push(...tree(own));
    // (Each other task's, under a head of its own.)
    for (const [t, list] of others) {
      out.push(el("div", { className: "head", title: "a task that ran while it did (one it woke, or waited for), and first wrote these" },
        `By ${t}, while it ran: ${count(list)}: ${names(list)}`));
      out.push(...tree(list));
    }
    // (Then those left as they were, under a head of their own.)
    if (same.length) {
      out.push(el("div", { className: "dim head", title: "changed, then changed back by the end (a lock taken and released, a count taken and dropped)" },
        `Left as it was, changed and changed back: ${names(same)}`));
      out.push(...tree(same));
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
          q === runs.length - 1 && to ? el("a", { textContent: called(to), onclick: (e) => (e.stopPropagation(), flash(d.to)) }) : r.after),
        el("td", { className: "ftype" }, r.at?.func ?? ""))));
    });
    // (A global: where it is declared, a link to it in the source.)
    const decl = n.kind === "global" ? m.definition(n.what) : null;
    // (The member it hangs from, unless its name says it: prog(19)->real_cred.)
    const via = n.via ? `  ← ${n.via}` : "";
    return el("div", { className: `obj ${n.kind}`, dataset: { k } },
      // (What it is, "file" (its type's name; the task's, a global's own),
      // then how the code names it, "filp in do_file_open", or its type.)
      el("div", { className: "title" }, el("b", { textContent: n.label }),
        el("span", { className: "dim", textContent: `  ${n.kind === "object" ? named(n) : n.ty}  at 0x${n.addr.toString(16)}${via}` }),
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
  return { draw, mark: (i) => $("right").dataset.tab === "objects" && mark(i ?? app.time.cursor()) };
}
