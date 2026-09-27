// The Objects: the kernel's objects the selected call (a syscall, or a call
// clicked in the timeline) changed, said as the kernel names them ("fd 3
// note.txt", "inode 12 (ext4)"; Machine.changes: what the code there held,
// a call's pointer argument, a local, the task), each with its writes that
// changed something (the member, from what to what, in which function;
// each a link, the cursor's marked, the view follows the pointer over one)
// and, opened, its value at the cursor as a tree. Pin one (📌, or click a
// value in the Stack, or an example's `watch`) to keep it as the selection
// changes, with its writes over the whole recording (a local's: while its
// call lives). The kernel's objects and real changes only; "all" shows the
// registers, frames, locals and unchanged writes too.
import { $, el } from "./dom.js";
import { tree, place } from "./tree.js";

export function objects(app) {
  const { m } = app;
  const body = $("objects");
  body.onpointerleave = () => app.peek(null);
  // Pinned: [{ o (its Object: name, the expression it was pinned as), life
  // ({ from, to, what }, or null: the whole recording) }]; the selection's
  // changes (Machine.changes); what is open (a value's tree, by key); all.
  const pinned = [], st = { open: new Set(), last: new Map(), now: new Map() };
  const values = tree(app, st, draw, add);
  let shown = null, changed = [], all = false;

  // Plumbing, not a change of the kernel's state: the entry's registers,
  // the frames the calls used (their spills), locals.
  const plumbing = (c) => /^struct pt_regs\b/.test(c.what) || /'s frame$/.test(c.what) || c.call != null;
  const key = (o) => `${o.addr}:${o.id}`;
  const real = (writes) => writes.filter((x) => x.before !== x.after);

  // Pin `o` (again: it moves to the top); `quiet`: without showing the tab.
  function add(o, life, quiet = false) {
    const k = pinned.findIndex((w) => key(w.o) === key(o) && w.life?.from === life?.from);
    if (k >= 0) pinned.splice(k, 1);
    pinned.unshift({ o, life });
    draw(true);
    if (!quiet) $("right").dataset.tab = "objects";
  }
  const unpin = (k) => (pinned.splice(k, 1), draw(true));

  // Draw: the selection's changes again only if it, or the snapshot,
  // changed (`force`: the pins did).
  function draw(force = false) {
    const w = app.time.home();
    const k = w ? `${w.a}:${w.b}:${w.task}:${m.record_id(m.records() - 1)}` : null;
    if (k !== shown) {
      shown = k;
      changed = w ? m.changes(w.a, w.b) : [];
    } else if (!force && body.children.length) return render();
    render();
  }

  function render() {
    st.last = st.now;
    st.now = new Map();
    const cur = app.time.cursor() ?? Infinity;
    // The pinned first, then the selection's changed objects not pinned.
    const kept = changed.filter((c) => c.addr && (all || !plumbing(c))).map((c) => ({ c, writes: all ? c.writes : real(c.writes) }))
      .filter((x) => x.writes.length && !pinned.some((p) => key(p.o) === key(x.c)));
    const hidden = changed.filter((c) => c.addr).length - kept.length - pinned.filter((p) => changed.some((c) => key(c) === key(p.o))).length;
    const head = el("div", { className: "dim head" },
      `${kept.length + pinned.length} object${kept.length + pinned.length === 1 ? "" : "s"}${pinned.length ? `, ${pinned.length} pinned` : ""}`,
      ...(hidden > 0 || all ? [" · ", el("a", { textContent: all ? "the kernel's objects" : `all (${hidden} more: registers, frames, locals, unchanged writes)`,
        onclick: () => ((all = !all), render()) })] : []));
    const items = [
      ...pinned.map((p, k) => entry({ o: p.o, what: m.describe(p.o.id, p.o.addr) ?? p.o.name, life: p.life, pin: () => unpin(k), pinned: true,
                                      writes: real(m.writes(p.o.addr, p.o.id, p.life?.from, p.life?.to)), scope: p.life?.what ?? "in the recording" }, cur)),
      ...kept.map(({ c, writes }) => entry({ o: asObject(c), what: c.what, life: c.call == null ? null : { from: c.call, to: m.call_end(c.call) ?? undefined, what: `while ${c.what.split("'s ")[0]}'s frame lives` },
                                            pin: () => add(asObject(c), c.call == null ? null : { from: c.call, to: m.call_end(c.call) ?? undefined, what: `while ${c.what.split("'s ")[0]}'s frame lives` }),
                                            writes, scope: "in this call" }, cur)),
    ];
    body.replaceChildren(head, ...items);
    $("objects-note").textContent = items.length ? "" : "Select a call (click one in the program, or a syscall in the timeline): the objects it changed. Click a value in the Stack to pin it here.";
  }
  // A changed object as an Object to pin: "f's x" as it is, a struct as
  // what its pointer points to.
  const asObject = (c) => ({ name: /^(struct|union) /.test(c.what) ? `*(${c.ty} *)0x${c.addr.toString(16)}` : c.what, ty: c.ty, id: c.id, addr: c.addr });

  // One object: its head (what it is, its type, ◀ ▶ to its writes around
  // the cursor, 📌), its value at the cursor (opened), its writes.
  function entry({ o, what, life, pin, pinned: isPinned, writes, scope }, cur) {
    const k = key(o), open = st.open.has(k);
    const alive = !life || (cur >= life.from && (life.to == null || cur <= life.to));
    const before = writes.findLast((x) => x.i < cur), after = writes.find((x) => x.i > cur), now = writes.findLastIndex((x) => x.i <= cur);
    const go = (x, text, which) => el("button", { textContent: text, disabled: !x, title: x ? `its ${which} write: ${place(x.at)}` : `no ${which} write`, onclick: () => app.time.go(x.i) });
    const head = el("div", { className: "whead" },
      el("a", { className: "tw", textContent: open ? "▾" : "▸", title: open ? "its value: fold" : "its value at the cursor", onclick: () => (open ? st.open.delete(k) : st.open.add(k), render()) }),
      el("b", { className: "mono", textContent: what }), el("span", { className: "dim mono", textContent: ` ${o.ty}` }),
      el("span", { className: "grow" }), go(before, "◀", "last"), go(after, "▶", "next"),
      el("a", { className: isPinned ? "pin on" : "pin", textContent: "📌", title: isPinned ? "unpin: only while selected" : "pin: keep it as the selection changes", onclick: pin }));
    const rows = [];
    if (open && alive) {
      const v = m.object_by_id(o.name, o.id, o.addr, 1) ?? o;
      values.rows([v], `o${k}`, 0, rows, life, o.name.startsWith("*") ? "" : o.name.slice(0, -v.name.length));
    }
    const list = el("ol", {}, ...writes.map((x, j) => el("li", { className: j === now ? "on" : "", dataset: { i: x.i } },
      el("a", { textContent: `${x.member ? `${x.member}: ` : ""}${x.before} → ${x.after}`, title: "go to this write", onclick: () => app.time.go(x.i), onpointerenter: () => app.peek(x.i) }),
      el("span", { className: "dim", textContent: ` in ${x.at?.func ?? "?"}` }))));
    return el("div", { className: `changed${isPinned ? " pinned" : ""}` }, head,
      open && !alive ? el("div", { className: "dim note", textContent: `not alive at the cursor (${life.what})` }) : null,
      rows.length ? el("table", { className: "objs" }, el("tbody", {}, ...rows)) : null,
      el("div", { className: "dim note", textContent: `${writes.length}${writes.length >= 200 ? "+" : ""} change${writes.length === 1 ? "" : "s"} ${scope}` }),
      writes.length ? list : null);
  }

  // The cursor moved: the writes at it marked; a pinned value's tree redrawn.
  function mark() {
    const cur = app.time.cursor();
    if (cur == null) return;
    if (pinned.some((p) => st.open.has(key(p.o)))) return render();
    for (const ol of body.querySelectorAll("ol")) {
      let on = null;
      for (const li of ol.children) if (Number(li.dataset.i) <= cur) on = li;
      ol.querySelector("li.on")?.classList.remove("on");
      on?.classList.add("on");
    }
  }
  // A new recording: the pins' records are gone.
  function reset() {
    pinned.length = 0;
    shown = null;
    draw(true);
  }
  return { add, draw, mark, reset };
}
