// The Records: the raw trace around the cursor (Machine.raw: a line a
// record, as the recording has them: blocks, arguments, locals, stores,
// returns; a task switch as a line), the cursor's record marked; click one
// to go there. Scroll past either end for more: the window grows. What
// every other view is built from.
import { $, el } from "./dom.js";

const AROUND = 60, MORE = 120;

export function records(app) {
  const { m } = app;
  // The records shown: `from..=to`, of recording `key`.
  const w = { from: 0, to: -1, key: null };
  const body = $("records");

  const row = (i, text, cur) => el("tr", { className: (i === cur ? "on " : "") + (text.startsWith("──") ? "task" : text.split(" ")[0]),
                                            dataset: { i }, onclick: () => app.time.go(i) },
    el("td", { className: "dim", textContent: i }), el("td", { textContent: text }));
  const note = () => ($("records-note").textContent = `records ${w.from}–${w.to} of ${m.records()}; scroll past an end for more`);

  // Draw for the cursor: the window around it, or, if it is in the
  // window, the mark moved.
  function draw() {
    const cur = app.time.cursor(), key = m.record_id(m.records() - 1);
    if (cur == null) {
      $("records-note").textContent = "The raw trace: pick a moment in the recording.";
      body.replaceChildren();
      w.key = null;
      return;
    }
    if (key === w.key && cur >= w.from && cur <= w.to) {
      body.querySelector("tr.on")?.classList.remove("on");
      const tr = body.querySelector(`tr[data-i="${cur}"]`);
      tr?.classList.add("on");
      tr?.scrollIntoView({ block: "nearest" });
      return;
    }
    w.key = key;
    w.from = Math.max(0, cur - AROUND);
    w.to = Math.min(m.records() - 1, cur + AROUND);
    body.replaceChildren(el("table", { className: "raw mono" }, el("tbody", {}, ...m.raw(w.from, w.to).map(([i, t]) => row(i, t, cur)))));
    note();
    requestAnimationFrame(() => body.querySelector("tr.on")?.scrollIntoView({ block: "center" }));
  }

  // More at the end scrolled past (the scroll kept where it is).
  body.onscroll = () => {
    const tbody = body.querySelector("tbody"), cur = app.time.cursor();
    if (!tbody) return;
    if (body.scrollTop < 40 && w.from > 0) {
      const from = Math.max(0, w.from - MORE), before = body.scrollHeight;
      tbody.prepend(...m.raw(from, w.from - 1).map(([i, t]) => row(i, t, cur)));
      w.from = from;
      body.scrollTop += body.scrollHeight - before;
      note();
    } else if (body.scrollTop + body.clientHeight > body.scrollHeight - 40 && w.to < m.records() - 1) {
      const to = Math.min(m.records() - 1, w.to + MORE);
      tbody.append(...m.raw(w.to + 1, to).map(([i, t]) => row(i, t, cur)));
      w.to = to;
      note();
    }
  };
  return { draw };
}
