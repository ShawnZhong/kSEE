// The Trace tab: the whole raw trace, a line a record, as the recording has
// them (a call with its arguments, a block, a store, a return, a task
// switch): what every other view is built from, for checking them and
// for seeing what the compiler recorded. A virtual list: the rows in view
// are built (Machine.raw), the rest is height. The cursor's row is marked
// in place, and scrolled to only when it is out of view; click a row to
// go there.
import { $, el } from "./dom.js";

const ROW = 18, OVER = 30; // px a row; rows built beyond the view

export function records(app) {
  const { m } = app;
  const body = $("records");
  const list = el("div", { className: "raw mono" }), rows = el("div", { className: "rows" });
  list.append(rows);
  body.replaceChildren(list);
  body.onscroll = () => fill();
  let built = [-1, -1], n = 0, snapshot = null, marked = null;

  // Redraw for the recording (its count changed: a new one, or grown).
  function draw() {
    const k = m.records(), id = k ? m.record_id(k - 1) : null;
    if (id !== snapshot) {
      snapshot = id;
      n = k;
      list.style.height = `${n * ROW}px`;
      built = [-1, -1];
      $("records-note").textContent = n ? `the trace: ${n} records` : "The raw trace: no recording yet.";
    }
    if ($("right").dataset.tab !== "records") return;
    fill();
    mark(undefined, true);
  }

  // Build the rows for the records in view (and a margin), if not built.
  function fill() {
    if (!n) return rows.replaceChildren();
    const from = Math.max(0, Math.floor(body.scrollTop / ROW) - OVER);
    const to = Math.min(n - 1, Math.ceil((body.scrollTop + body.clientHeight) / ROW) + OVER);
    if (from >= built[0] && to <= built[1]) return;
    built = [from, to];
    const cur = app.time.cursor();
    rows.style.top = `${from * ROW}px`;
    rows.replaceChildren(...m.raw(from, to).map(([i, kind, text]) => el("div", {
      className: `row ${kind}${i === cur ? " on" : ""}`, dataset: { i },
      onclick: () => app.time.go(i) },
      el("span", { className: "dim", textContent: i }), el("span", { textContent: text }))));
    marked = cur;
  }

  // The cursor moved: its row marked; scrolled into view if out of it.
  function mark(i, force = false) {
    const cur = i ?? app.time.cursor();
    if ($("right").dataset.tab !== "records" || !n) return;
    if (cur != null && (cur * ROW < body.scrollTop || (cur + 1) * ROW > body.scrollTop + body.clientHeight)) {
      body.scrollTop = cur * ROW - body.clientHeight / 2;
      fill();
    }
    if (cur === marked && !force) return;
    rows.querySelector(".row.on")?.classList.remove("on");
    rows.querySelector(`.row[data-i="${cur}"]`)?.classList.add("on");
    marked = cur;
  }
  return { draw, mark };
}
