// The Records: the raw trace around the cursor (Machine.raw: a line a
// record, as the recording has them: blocks, arguments, locals, stores,
// returns; a task switch as a line), the cursor's record marked; click one
// to go there. What every other view is built from.
import { $, el } from "./dom.js";

const AROUND = 60;

export function records(app) {
  const { m } = app;
  function draw() {
    const cur = app.time.cursor();
    if (cur == null) {
      $("records-note").textContent = "The raw trace: pick a moment in the recording.";
      $("records").replaceChildren();
      return;
    }
    const from = Math.max(0, cur - AROUND), to = Math.min(m.records() - 1, cur + AROUND);
    $("records-note").textContent = `records ${from}–${to} of ${m.records()}, the cursor at ${cur}`;
    const rows = m.raw(from, to).map(([i, text]) => el("tr", { className: (i === cur ? "on " : "") + (text.startsWith("──") ? "task" : text.split(" ")[0]), onclick: () => app.time.go(i) },
      el("td", { className: "dim", textContent: i }), el("td", { textContent: text })));
    $("records").replaceChildren(el("table", { className: "raw mono" }, el("tbody", {}, ...rows)));
    requestAnimationFrame(() => $("records").querySelector("tr.on")?.scrollIntoView({ block: "center" }));
  }
  return { draw };
}
