// The Tasks: every task the recording has, at the cursor (Machine.tasks),
// in the order they first run: the one running marked, each where it is
// (the syscall it is in, else its outermost call, as it last ran); its
// name a link to that moment (the Stack then shows its stack). Open: its
// task_struct as it was at the cursor, its key fields, all of them on
// asking, as the Stack's values are.
import { $, el, taskName } from "./dom.js";
import { tree } from "./tree.js";

export function tasks(app) {
  const { m } = app;
  const body = $("tasks");
  let shown = null;
  // (The tasks open, by address: "task/<addr>", and "…/all" for all of it.)
  const st = { open: new Set(), last: new Map(), now: new Map() };
  const redraw = () => ((shown = null), draw());
  const { rows } = tree(app, st, redraw);
  const toggle = (p) => () => (st.open.has(p) ? st.open.delete(p) : st.open.add(p), redraw());

  // Draw at the cursor (only while the tab is shown: again as it moves).
  function draw() {
    if ($("right").dataset.tab !== "tasks") return;
    const cur = app.time.cursor();
    if (cur === shown) return;
    shown = cur;
    $("tasks-note").textContent = cur == null ? "Nothing recorded yet: the tasks come from the recording. Make a call." : "";
    if (cur == null) return body.replaceChildren();
    st.last = st.now;
    st.now = new Map();
    const all = [];
    for (const t of m.tasks(cur)) {
      const p = `task/${t.task.addr}`, open = st.open.has(p);
      // (The one running, marked; one not run yet by the cursor, dim.)
      all.push(el("tr", { className: `frame task${t.running ? " on" : ""}${t.last == null ? " later" : ""}`,
                          title: t.running ? "running at the cursor" : t.last == null ? "runs only after the cursor" : "" }, el("td", { colSpan: 3 },
        el("a", { className: "tw", textContent: open ? "▾ " : "▸ ", title: open ? "fold it" : "its task_struct, as it was at the cursor", onclick: toggle(p) }),
        el("a", { className: "mono", textContent: taskName(t.task), title: t.running ? "" : "go to its last moment (its first, if it runs later)",
                  onclick: () => app.time.go(t.last ?? t.first) }),
        el("span", { className: "dim", textContent: t.last == null ? "" : `  in ${t.syscall ?? t.func ?? "?"}` }))));
      if (!open) continue;
      const full = st.open.has(`${p}/all`), { fields, total } = m.task_fields(t.task.addr, full);
      rows(fields, p, 1, all);
      all.push(el("tr", { className: "sub" }, el("td", { colSpan: 3 },
        el("a", { textContent: full ? "▾ the key fields only" : `▸ all ${total} fields`, onclick: toggle(`${p}/all`) }))));
    }
    body.replaceChildren(el("table", { className: "objs stack" }, el("tbody", {}, ...all)));
  }

  // The Tasks tab, task `addr` open (the Stack's current task: a link here).
  function open(addr) {
    st.open.add(`task/${addr}`);
    $("right").dataset.tab = "tasks";
    redraw();
  }

  return { draw, mark: draw, open };
}
