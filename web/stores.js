// The Stores: every store the selected call (a syscall, or a call clicked
// in the timeline) made, a row each, in order (Machine.stores: named as
// the code names what it hit, "vfs_read's file->f_pos", a global by its
// name, a local as "f's x", else by its address, with what it was and
// what it became and where: a link to that moment). The store in effect
// at the cursor is marked and the ones after it are dim, so the list
// reads as the call's progress; "here" scrolls to it. Three or more stores
// in a row to one object are one row, open on asking (a frame zeroed, a
// struct set up: the plumbing, folded).
import { $, el } from "./dom.js";
import { place } from "./tree.js";

const TAG = { local: "local", frame: "frame", machine: "the machine's" };
// A store said: "vfs_read's file->f_pos: 0 → 26".
// (A frame's bytes by their offset: "f's frame+8".)
const joint = (kind, member) => (member.startsWith("+") ? "" : kind === "object" ? "->" : ".");
export const storeText = ({ what, kind, write: x }) => `${what}${x.member ? joint(kind, x.member) + x.member : ""}: ${x.before} → ${x.after}`;

export function stores(app) {
  const body = $("stores");
  let shown = null, rows = [];
  const opened = new Set(); // (the runs open: their first store's record)

  // Draw for the selection (again only if it, or the snapshot, changed).
  function draw() {
    const w = app.selection(), key = w?.key ?? null;
    if (key === shown) return;
    shown = key;
    rows = app.selectionStores();
    opened.clear();
    $("stores-note").textContent = w ? "" : "Select a call (click one in the program, or a syscall in the timeline): its stores, in order.";
    render();
  }

  function render() {
    body.replaceChildren(
      el("div", { className: "dim head" }, `${rows.length} store${rows.length === 1 ? "" : "s"} · `,
        el("a", { textContent: "here", title: "the store in effect at the cursor", onclick: () => body.querySelector("li.on")?.scrollIntoView({ block: "center" }) })),
      el("ol", {}, ...runs().flatMap((run) => {
        if (run.length < 3) return run.map(item);
        // (A run: one row, its stores under it when open.)
        const [{ what, kind, write: x }] = run, open = opened.has(x.i);
        const members = [...new Set(run.map((s) => s.write.member || "(all of it)"))];
        const head = el("li", { dataset: { i: x.i }, className: "run" },
          el("a", { className: "tw", textContent: open ? "▾" : "▸", onclick: () => (open ? opened.delete(x.i) : opened.add(x.i), render()) }),
          el("a", { className: "mono", textContent: `${what}: ${run.length} stores (${members.slice(0, 4).join(", ")}${members.length > 4 ? ", …" : ""})`,
                    title: "go to its first store", onclick: () => app.time.go(x.i) }),
          el("span", { className: "dim", textContent: ` ${place(x.at)}${TAG[kind] ? ` · ${TAG[kind]}` : ""}` }));
        return open ? [head, ...run.map(item)] : [head];
      })));
    mark();
  }
  // Runs of stores to one object, in a row.
  function runs() {
    const out = [];
    for (const s of rows) {
      const last = out.at(-1)?.at(-1);
      if (last && last.what === s.what && last.kind === s.kind) out.at(-1).push(s);
      else out.push([s]);
    }
    return out;
  }
  // A store's row. (One that left the value as it was: dim.)
  const item = ({ what, kind, ty, write: x }) => el("li", { dataset: { i: x.i }, className: x.before === x.after ? "same" : "" },
    el("a", { className: "mono", textContent: storeText({ what, kind, write: x }), title: `go to this store${ty ? ` (${ty})` : ""}`, onclick: () => app.time.go(x.i) }),
    el("span", { className: "dim", textContent: ` ${place(x.at)}${TAG[kind] ? ` · ${TAG[kind]}` : ""}` }));

  // The cursor moved: the store in effect (the last at or before the
  // cursor) marked, the ones after dim.
  function mark(i) {
    const cur = i ?? app.time.cursor() ?? -1;
    let on = null;
    for (const li of body.querySelectorAll("li")) {
      const i = Number(li.dataset.i);
      li.classList.toggle("later", i > cur);
      if (i <= cur) on = li;
    }
    body.querySelector("li.on")?.classList.remove("on");
    on?.classList.add("on");
  }
  return { draw, mark };
}
