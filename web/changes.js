// The Changes: what the selected call (a syscall, or a call clicked in the
// timeline) changed, by object (Machine.changes: the objects the code
// there held, a call's pointer argument to a struct, a local, the task),
// said as the kernel names them ("fd 3 note.txt", "inode 12 (ext4)"),
// each with its writes: the member, from what to what, by which function
// (a link to that moment; the view follows the pointer over one). Only
// what a learner would call a change: the kernel's own objects (not the
// syscall entry's registers, nor the stack frames the calls used), and
// writes that changed a value; "all" shows the rest too. Click an object
// to watch it.
import { $, el } from "./dom.js";

export function changes(app, watch) {
  const { m } = app;
  const body = $("changes");
  body.onpointerleave = () => app.peek(null);
  let shown = null, list = [], all = false;

  // Plumbing, not a change of the kernel's state: the entry's registers,
  // the frames the calls used (their spills), locals.
  const plumbing = (c) => /^struct pt_regs\b/.test(c.what) || /'s frame$/.test(c.what) || c.call != null;

  // Draw for the selection (again only if it, or the snapshot, changed).
  function draw() {
    const w = app.time.home();
    const key = w ? `${w.a}:${w.b}:${w.task}:${m.record_id(m.records() - 1)}` : null;
    if (key === shown) return;
    shown = key;
    $("changes-note").textContent = w ? "" : "Select a call (click one in the program, or a syscall in the timeline): what it changed, by object.";
    if (!w) return body.replaceChildren();
    list = m.changes(w.a, w.b);
    render();
  }

  function render() {
    const cur = app.time.cursor() ?? Infinity;
    const objects = list.filter((c) => c.addr && (all || !plumbing(c)));
    const shownOf = (c) => (all ? c.writes : c.writes.filter((x) => x.before !== x.after));
    const kept = objects.map((c) => ({ c, writes: shownOf(c) })).filter((x) => x.writes.length);
    const hidden = list.filter((c) => c.addr).length - kept.length;
    const head = el("div", { className: "dim head" },
      kept.length ? `${kept.length} object${kept.length === 1 ? "" : "s"} changed` : "nothing changed",
      ...(hidden > 0 || all ? [" · ", el("a", { textContent: all ? "the kernel's objects" : `all (${hidden} more: registers, frames, locals, unchanged writes)`,
        onclick: () => ((all = !all), render()) })] : []));
    body.replaceChildren(head, ...kept.map(({ c, writes }) => {
      const now = writes.findLastIndex((x) => x.i <= cur);
      return el("div", { className: "changed" },
        el("div", { className: "whead" },
          // (Watched as what it is: "f's x", or "*(struct file *)0x1234"; a
          // local or a frame for as long as its call lives.)
          el("a", { className: "mono", textContent: c.what, title: "watch it: its value as the cursor moves, every write to it",
                    onclick: () => watch({ name: /^(struct|union) /.test(c.what) ? `*(${c.ty} *)0x${c.addr.toString(16)}` : c.what, ty: c.ty, id: c.id, addr: c.addr },
                                         c.call == null ? null : { from: c.call, to: m.call_end(c.call) ?? undefined, what: `while ${c.what.split("'s ")[0]}'s frame lives` }) }),
          el("span", { className: "dim", textContent: ` ${writes.length} write${writes.length === 1 ? "" : "s"}${c.n > c.writes.length ? ` (of the last ${c.writes.length} of ${c.n})` : ""}` })),
        el("ol", {}, ...writes.map((x, j) => el("li", { className: j === now ? "on" : "", dataset: { i: x.i } },
          el("a", { textContent: `${x.member ? `${x.member}: ` : ""}${x.before} → ${x.after}`, title: "go to this write", onclick: () => app.time.go(x.i), onpointerenter: () => app.peek(x.i) }),
          // (By which function: the one whose code it is, not the inlined
          // helper the line is from.)
          el("span", { className: "dim", textContent: x.at ? ` in ${x.at.func}` : "" })))));
    }));
  }

  // The cursor moved: the write at it marked, if shown.
  function mark() {
    const cur = app.time.cursor();
    if (cur == null) return;
    for (const ol of body.querySelectorAll("ol")) {
      let on = null;
      for (const li of ol.children) {
        const i = Number(li.dataset.i);
        if (i <= cur) on = li;
      }
      ol.querySelector("li.on")?.classList.remove("on");
      on?.classList.add("on");
    }
  }
  return { draw, mark };
}
