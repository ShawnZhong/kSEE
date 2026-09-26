// The Changes: what the selected call (the timeline's selection: a syscall,
// or a call clicked in the chart) changed, by object (Machine.changes: a
// struct a call got a pointer to, a local, the task running, each said as
// what it is where the kernel's fields say: "fd 3 /note.txt", "inode 12
// (ext4)"; "other" the stores to memory nothing names), in the order of
// their first write,
// each with its writes: the member, from what to what, by which line,
// each a link to that moment (the view follows the pointer over one:
// app.peek). Click an object to watch it (watch.js).
import { $, el } from "./dom.js";
import { place } from "./objects.js";

export function changes(app, watch) {
  const { m } = app;
  let key = null, list = [];
  $("changes").onpointerleave = () => app.peek(null);

  // Draw for the selection (again only if it changed); the write in effect
  // at the cursor marked.
  function draw() {
    const h = app.time.home(), cur = app.time.cursor() ?? Infinity;
    const k = h ? `${h.a}:${h.b}:${m.record_id(m.records() - 1)}` : null;
    if (k !== key) {
      key = k;
      list = h ? m.changes(h.a, h.b) : [];
    }
    $("changes-note").textContent = !h ? "What a call changed: select a call (a syscall in the program, or a box in the timeline)."
      : list.length ? `${$("chart-title").textContent}: ${list.reduce((n, c) => n + c.n, 0)} writes to ${list.filter((c) => c.addr).length} objects`
      : "no writes in the selection";
    $("changes").replaceChildren(...list.map((c) => {
      if (!c.addr) return el("div", { className: "changed dim note", textContent: `and ${c.n} stores to memory no call here got by argument or local (the heap through locals, bulk copies)` });
      const now = c.writes.findLastIndex((x) => x.i <= cur);
      const items = c.writes.map((x, j) => el("li", { className: j === now ? "on" : "" },
        el("a", { textContent: `${x.member ? `${x.member}: ` : ""}${x.before} → ${x.after}`, onclick: () => app.time.go(x.i), onpointerenter: () => app.peek(x.i) }),
        el("span", { className: "dim", textContent: ` ${place(x.at)}` })));
      return el("div", { className: "changed" },
        el("div", { className: "whead" },
          // (Watched as what it is: "f's x", or "*(struct file *)0x1234".)
          el("a", { className: "mono", textContent: c.what, title: "watch it",
                    onclick: () => watch({ name: /^(struct|union) /.test(c.what) ? `*(${c.ty} *)0x${c.addr.toString(16)}` : c.what, ty: c.ty, id: c.id, addr: c.addr }, null) }),
          el("span", { className: "dim", textContent: ` ${c.n} write${c.n === 1 ? "" : "s"}${c.n > c.writes.length ? `, the last ${c.writes.length}` : ""}` })),
        el("ol", {}, ...items));
    }));
  }
  return { draw };
}
