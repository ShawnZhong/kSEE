// The Changes: what the selected call (a syscall, or a call clicked in the
// timeline) changed, by object, in the order they were first written
// (Machine.changes: the structs the call's arguments point to, the task,
// the kernel's globals; then, under a divider, its own locals, frames,
// registers and the machine's variables). Each write a row: the member, from what to what,
// in which function at which line, a link to that moment; the write in
// effect at the cursor is marked and the ones after it are dim, so the
// list reads as the call's progress. "here" scrolls to the cursor's.
import { $, el } from "./dom.js";
import { place } from "./tree.js";

const TAG = { task: "", object: "", global: "global", local: "local", frame: "frame", regs: "registers", machine: "the machine's", other: "" };

export function changes(app) {
  const { m } = app;
  const body = $("changes");
  let shown = null, list = [];

  // Draw for the selection (again only if it, or the snapshot, changed).
  function draw() {
    const w = app.time.home();
    const key = w ? `${w.a}:${w.b}:${w.task}:${m.record_id(m.records() - 1)}` : null;
    if (key === shown) return;
    shown = key;
    list = w ? m.changes(w.a, w.b) : [];
    $("changes-note").textContent = w ? "" : "Select a call (click one in the program, or a syscall in the timeline): what it changed, by object.";
    render();
  }

  const own = (c) => ["local", "frame", "regs", "machine"].includes(c.kind);
  function render() {
    const objects = list.filter((c) => c.addr), other = list.find((c) => !c.addr);
    const kernel = objects.filter((c) => !own(c)), rest = objects.filter(own);
    const writes = objects.reduce((a, c) => a + c.n, 0);
    body.replaceChildren(
      el("div", { className: "dim head" }, `${objects.length} object${objects.length === 1 ? "" : "s"}, ${writes} write${writes === 1 ? "" : "s"} · `,
        el("a", { textContent: "here", title: "the write in effect at the cursor", onclick: () => body.querySelector("li.on")?.scrollIntoView({ block: "center" }) })),
      ...kernel.map(entry),
      rest.length ? el("div", { className: "dim note", textContent: "the call's own: locals, frames, registers, the machine's variables" }) : null,
      ...rest.map(entry),
      other ? el("div", { className: "dim note", textContent: `and ${other.n} store${other.n === 1 ? "" : "s"} to memory nothing here names` }) : null);
    mark();
  }
  function entry(c) {
    return el("div", { className: "changed" },
        el("div", { className: "whead" }, el("b", { className: "mono", textContent: c.what }), el("span", { className: "dim mono", textContent: ` ${c.ty}` }),
          TAG[c.kind] ? el("span", { className: "dim", textContent: ` · ${TAG[c.kind]}` }) : null,
          c.n > c.writes.length ? el("span", { className: "dim", textContent: ` · ${c.n} writes, the last ${c.writes.length}` }) : null),
        el("ol", {}, ...c.writes.map((x) => el("li", { dataset: { i: x.i } },
          el("a", { textContent: `${x.member ? `${x.member}: ` : ""}${x.before} → ${x.after}`, title: "go to this write", onclick: () => app.time.go(x.i) }),
          el("span", { className: "dim", textContent: ` ${place(x.at)}` })))));
  }

  // The cursor moved: in each object's writes, the one in effect (the last
  // at or before the cursor) marked, the ones after dim.
  function mark() {
    const cur = app.time.cursor() ?? -1;
    for (const ol of body.querySelectorAll("ol")) {
      let on = null;
      for (const li of ol.children) {
        const i = Number(li.dataset.i);
        li.classList.toggle("later", i > cur);
        if (i <= cur) on = li;
      }
      ol.querySelector("li.on")?.classList.remove("on");
      on?.classList.add("on");
    }
  }
  return { draw, mark };
}
