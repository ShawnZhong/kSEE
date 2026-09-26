// The Watch: values clicked in the Call stack (or in a watched value), each
// kept as the cursor moves: the object (at its address, of its type), its
// value then, and its history (Machine.writes): every write to it in its
// lifetime (a local's: its frame's), the member it wrote, from what to
// what, by which line, each a link to that moment, the one in effect at the
// cursor marked; ◀ ▶ go to its write before or after the cursor.
import { $, el } from "./dom.js";
import { objects, place } from "./objects.js";

export function watch(app) {
  const { m } = app;
  // Watched: [{ o (its Object: name, the expression it was clicked as),
  // life ({ from, to, what }, or null: the whole recording), all (its
  // writes) }].
  const st = { open: new Set(), last: new Map(), now: new Map() }, list = [];
  const tree = objects(app, st, draw, add);

  // Watch `o` (again: it moves to the top).
  function add(o, life) {
    const k = list.findIndex((w) => w.o.addr === o.addr && w.o.id === o.id && w.life?.from === life?.from);
    if (k >= 0) list.splice(k, 1);
    list.unshift({ o, life, all: m.writes(o.addr, o.id, life?.from, life?.to) });
    draw();
  }

  function draw() {
    st.last = st.now;
    st.now = new Map();
    const cur = app.time.cursor() ?? Infinity;
    $("watch").replaceChildren(...list.map((w, k) => {
      const alive = !w.life || (cur >= w.life.from && (w.life.to == null || cur <= w.life.to));
      const before = w.all.findLast((x) => x.i < cur), after = w.all.find((x) => x.i > cur);
      const now = w.all.findLastIndex((x) => x.i <= cur);
      const go = (x, text, what) => el("button", { textContent: text, disabled: !x, title: x ? `its ${what} write: ${place(x.at)}` : `no ${what} write`,
                                                  onclick: () => app.time.go(x.i) });
      // (Said as what it is, when the kernel's fields say: "fd 3 /note.txt".)
      const said = alive ? m.describe(w.o.id, w.o.addr) : null;
      const head = el("div", { className: "whead" },
        el("b", { className: "mono", textContent: w.o.name }), el("span", { className: "dim mono", textContent: ` ${said ? `${said} · ` : ""}${w.o.ty}` }),
        el("span", { className: "grow" }), go(before, "◀", "last"), go(after, "▶", "next"),
        el("a", { className: "rm", textContent: "✕", title: "stop watching", onclick: () => (list.splice(k, 1), draw()) }));
      // (Its value at the cursor, as a tree; outside its lifetime, none.)
      const body = [];
      if (alive) {
        const o = m.object_by_id(w.o.name, w.o.id, w.o.addr, 1) ?? w.o;
        tree.rows([o], `w${w.o.addr}:${w.o.id}`, 0, body, w.life, w.o.name.slice(0, -o.name.length));
      }
      const items = w.all.map((x, j) => el("li", { className: j === now ? "on" : "" },
        el("a", { textContent: `${x.member ? `${x.member}: ` : ""}${x.before} → ${x.after}`, onclick: () => app.time.go(x.i) }),
        el("span", { className: "dim", textContent: ` ${place(x.at)} · ${x.task.comm ?? "?"}(${x.task.pid ?? "?"})` })));
      const ol = el("ol", {}, ...items);
      requestAnimationFrame(() => ol.querySelector("li.on")?.scrollIntoView({ block: "nearest" }));
      return el("div", { className: "watched" }, head,
        alive ? el("table", { className: "objs" }, el("tbody", {}, ...body))
          : el("div", { className: "dim note", textContent: `not alive at the cursor (${w.life.what})` }),
        el("div", { className: "dim note", textContent: `${w.all.length}${w.all.length >= 256 ? "+" : ""} writes ${w.life?.what ?? "in the recording"}` }),
        items.length ? ol : null);
    }));
    $("watch-note").textContent = list.length ? "" : "Click a value in the Call stack to watch it: its value as the cursor moves, every write to it.";
  }

  // A new recording: the watched values' records are gone.
  function reset() {
    list.length = 0;
    draw();
  }
  return { add, draw, reset };
}
