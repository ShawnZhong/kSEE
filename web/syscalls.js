// The syscalls in the trace, and what the selected one ran.
import { $, el, hex, duration, retText, taskName } from "./dom.js";

export function syscalls(app) {
  const { m } = app;
  let rows = [], list = [];

  // Re-read the trace ring and redraw the list, keeping the selection.
  function refresh() {
    m.snapshot();
    list = m.syscalls();
    const shown = list.filter((s) => $("show-agent").checked || !s.control);
    $("nsys").textContent = `${shown.length}`;
    rows = shown.map((s) => {
      const row = el("div", { className: "row" + (s.control ? " control" : ""), dataset: { index: s.index } },
        el("div", { className: "call", textContent: `${s.name ?? s.nr}(${s.args.map(hex).join(", ")}) = ${s.ret == null ? "…" : retText(s.ret)}` }),
        el("div", { className: "meta" },
          el("a", { textContent: taskName(s.task), onclick: (e) => {
            e.stopPropagation();
            app.inspect(s.task.comm, "task_struct", s.task.addr);
          } }),
          " · ", el("span", { className: s.duration_ns == null ? "live" : "", textContent: duration(s.duration_ns) }),
          ` · ${s.blocks} blocks, ${s.functions} functions`));
      row.onclick = () => show(s);
      return row;
    });
    $("syscalls").replaceChildren(...(rows.length ? rows : [el("div", { className: "empty", textContent: "No syscalls yet: make one above." })]));
    return list;
  }
  $("show-agent").onchange = refresh;

  // Show syscall `s`: its runs, and the function `fn` (or the first past
  // the entry) in the source, at line `line`.
  function show(s, fn, line) {
    rows.forEach((r) => r.classList.toggle("on", Number(r.dataset.index) === s.index));
    $("syscalls").querySelector(".row.on")?.scrollIntoView({ block: "nearest" });
    $("run-title").textContent = `${s.name} by ${taskName(s.task)}`;
    const runs = m.runs(s.index);
    const items = runs.map((r) => {
      const item = el("div", { className: "row run" }, el("span", { className: "mono", textContent: r.func }),
        el("span", { className: "count", textContent: `${r.blocks}` }), el("span", { className: "where", textContent: `${r.file}:${r.line}` }));
      item.onclick = () => select(item, r.func);
      return item;
    });
    $("runs").replaceChildren(...items);
    const select = (item, func, at) => {
      items.forEach((i) => i.classList.remove("on"));
      item.classList.add("on");
      item.scrollIntoView({ block: "nearest" });
      app.showFunction(func, s.index, at);
    };
    let i = fn ? runs.findLastIndex((r) => r.func === fn) : -1;
    if (i < 0) i = runs.findIndex((r) => r.func !== "wasm_syscall" && !r.func.startsWith("__wasm_sys_"));
    if (items[i] ?? items[0]) select(items[i] ?? items[0], (runs[i] ?? runs[0]).func, line);
  }

  const byIndex = (i) => list.find((s) => s.index === i);
  return { refresh, show, byIndex, all: () => list };
}
