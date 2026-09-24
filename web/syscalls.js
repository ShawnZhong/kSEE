// The syscalls in the trace (a list, in the Syscalls tab), and what the
// selected one ran: its call path, as runs of blocks per function.
import { $, el, hex, duration, retText, taskName } from "./dom.js";

export function syscalls(app) {
  const { m } = app;
  let rows = [], list = [], runItems = [], runs = [], shown = null;

  // Re-read the trace ring and redraw the list, keeping the selection.
  function refresh() {
    m.snapshot();
    list = m.syscalls();
    runItems = []; // (runs of a syscall in progress grow)
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
    if (shown) mark(app.key(shown));
    return list;
  }

  const mark = (key) => rows.forEach((r) => r.classList.toggle("on", r.dataset.key === key));

  // Show what syscall `s` ran; the run of `fn` (the last one, or the first
  // past the entry) selected, its source at `line`.
  function show(s, fn, line) {
    if (shown?.index !== s.index || !runItems.length) {
      shown = s;
      mark(app.key(s));
      $("run-title").textContent = `${s.name ?? s.nr} by ${taskName(s.task)}`;
      $("run-meta").textContent = `= ${s.ret == null ? "…" : retText(s.ret)} · ${duration(s.duration_ns)} · ${s.blocks} blocks`;
      runs = m.runs(s.index);
      runItems = runs.map((r) => {
        const item = el("div", { className: "row run" }, el("span", { className: "mono", textContent: r.func }),
          el("span", { className: "count", textContent: `${r.blocks}` }));
        item.title = `${r.file}:${r.line}`;
        item.onclick = () => pick(item, r.func);
        return item;
      });
      $("runs").replaceChildren(...runItems);
    }
    let i = fn ? runs.findLastIndex((r) => r.func === fn) : -1;
    if (i < 0) i = runs.findIndex((r) => r.func !== "wasm_syscall" && !r.func.startsWith("__wasm_sys_"));
    if (i < 0) i = 0;
    if (runItems[i]) pick(runItems[i], runs[i].func, line);
  }
  function pick(item, func, line) {
    runItems.forEach((i) => i.classList.toggle("on", i === item));
    item.scrollIntoView({ block: "nearest" });
    app.showFunction(func, shown.index, line);
  }

  const byIndex = (i) => list.find((s) => s.index === i);
  const byKey = (k) => list.find((s) => app.key(s) === k);
  return { refresh, show, byIndex, byKey, all: () => list };
}
