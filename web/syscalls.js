// The syscalls in the trace, newest first (the Syscalls tab).
import { $, el, hex, duration, retText, taskName } from "./dom.js";

export function syscalls(app) {
  const { m } = app;
  let rows = [], list = [];

  // Re-read the trace ring and redraw the list, keeping the selection.
  function refresh() {
    m.snapshot();
    list = m.syscalls();
    const visible = list.filter((s) => $("show-control").checked || !s.control);
    $("nsys").textContent = `${visible.length}`;
    rows = visible.map((s) => {
      const row = el("div", { className: "row" + (s.control ? " control" : ""), dataset: { key: app.key(s) } },
        el("div", { className: "call", textContent: `${s.name ?? s.nr}(${s.args.map(hex).join(", ")}) = ${s.ret == null ? "…" : retText(s.ret)}` }),
        el("div", { className: "meta" },
          el("a", { textContent: taskName(s.task), onclick: (e) => {
            e.stopPropagation();
            app.inspect(s.task.comm, "task_struct", s.task.addr);
          } }),
          " · ", el("span", { className: s.duration_ns == null ? "live" : "", textContent: duration(s.duration_ns) }),
          ` · ${s.blocks} blocks, ${s.functions} functions`));
      row.onclick = () => app.select(s);
      return row;
    }).reverse();
    $("syscalls").replaceChildren(...(rows.length ? rows : [el("div", { className: "empty", textContent: "No syscalls in the trace yet." })]));
    return list;
  }

  const mark = (key) => rows.forEach((r) => r.classList.toggle("on", r.dataset.key === key));
  const byIndex = (i) => list.find((s) => s.index === i);
  const byKey = (k) => list.find((s) => app.key(s) === k);
  return { refresh, mark, byIndex, byKey, all: () => list };
}
