// The Tasks: every task the recording has, at the cursor (Machine.tasks),
// in the order they first run: the one running marked, each where it is
// (the syscall it is in, else its outermost call, as it last ran); its
// name a link to that moment (the Stack then shows its stack). Open (as
// each is, to begin with): its task_struct as it was at the cursor, its key fields, all of them on
// asking, as the Stack's values are.
import { $, el, taskName } from "./dom.js";
import { tree } from "./tree.js";

export function tasks(app) {
  const { m } = app;
  const body = $("tasks");
  let shown = null, moment = null;
  // (The tasks folded, by address: "task/<addr>" (each is open to begin
  // with); open, "…/all" for all of its fields; the values' paths open.)
  const folded = new Set(), st = {};
  const redraw = () => ((shown = null), draw());
  const { rows, moved } = tree(app, st, redraw);
  const toggle = (set, p) => () => (set.has(p) ? set.delete(p) : set.add(p), redraw());

  // Draw at record `cur`: the moment the page shows (the cursor, or the
  // pointer's over the timeline; only while the tab is shown: again as it
  // moves).
  function draw(cur = app.time.cursor()) {
    if ($("right").dataset.tab !== "tasks") return;
    // (Again for another moment, or another recording: a snapshot.)
    const n = m.records(), key = `${cur}:${n ? m.record_id(n - 1) : ""}`;
    if (key === shown) return;
    // (Another moment: what changed since, marked; not on a redraw.)
    if (key !== moment) (moved(), (moment = key));
    shown = key;
    $("tasks-note").textContent = cur == null ? "Nothing recorded yet: the tasks come from the recording. Make a call." : "";
    if (cur == null) return body.replaceChildren();
    const all = [];
    for (const t of m.tasks(cur)) {
      const p = `task/${t.task.addr}`, open = !folded.has(p);
      // (The one running, marked; one not run yet by the cursor, dim.)
      all.push(el("tr", { className: `frame task${t.running ? " on" : ""}${t.last == null ? " later" : ""}`,
                          title: t.running ? "running at the cursor" : t.last == null ? "runs only after the cursor" : "" }, el("td", { colSpan: 3 },
        el("a", { className: "tw", textContent: open ? "▾ " : "▸ ", title: open ? "fold it" : "its task_struct, as it was at the cursor", onclick: toggle(folded, p) }),
        el("a", { className: "mono", textContent: taskName(t.task), title: t.running ? "" : "go to its last moment (its first, if it runs later)",
                  onclick: () => app.time.go(t.last ?? t.first) }),
        el("span", { className: "dim", textContent: t.last == null || !(t.syscall ?? t.func) ? "" : `  in ${t.syscall ?? t.func}` }))));
      if (!open) continue;
      const full = st.open.has(`${p}/all`), { fields, total } = m.task_fields(t.task.addr, full);
      rows(fields, p, 1, all);
      all.push(el("tr", { className: "sub" }, el("td", { colSpan: 3 },
        el("a", { textContent: full ? "▾ the key fields only" : `▸ all ${total} fields`, onclick: toggle(st.open, `${p}/all`) }))));
    }
    body.replaceChildren(el("table", { className: "objs stack" }, el("tbody", {}, ...all)));
  }

  // The Tasks tab, task `addr` open (the Stack's current task: a link here).
  function open(addr) {
    folded.delete(`task/${addr}`);
    app.tab("tasks");
    redraw();
  }

  return { draw: () => draw(), mark: (i) => draw(i ?? app.time.cursor()), open };
}
