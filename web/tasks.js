// The Tasks: every task the recording has, at the cursor (Machine.tasks),
// as a tree (a thread under its process's leader, a process under its
// parent): the one running marked, each where it is
// (the syscall it is in, else its outermost call, as it last ran); its
// name a link to that moment (the Stack then shows its stack). Open (the
// running one, to begin with): its task_struct as it was at the cursor,
// its key fields first, then the others.
import { $, el, taskName } from "./dom.js";
import { tree } from "./tree.js";

export function tasks(app) {
  const { m } = app;
  const body = $("tasks");
  let shown = null, moment = null;
  // (The tasks opened or folded, by address, "task/<addr>" (the running
  // one is open to begin with, the others folded); the values' paths open.)
  const opened = new Map(), st = {};
  const redraw = () => ((shown = null), draw());
  const { rows, moved } = tree(app, st, redraw);

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
    // (As a tree: each under the task it sits under (a thread, its leader; a
    // process, its parent), siblings by pid; the ones whose parent the
    // recording does not have, at the top.)
    const list = m.tasks(cur), known = new Set(list.map((t) => t.task.addr));
    const under = (up) => list.filter((t) => (known.has(t.up) ? t.up : null) === up).sort((a, b) => (a.task.pid ?? 0) - (b.task.pid ?? 0));
    const order = [];
    const visit = (t, depth) => (order.push([t, depth]), under(t.task.addr).forEach((c) => visit(c, depth + 1)));
    under(null).forEach((t) => visit(t, 0));
    for (const [t, depth] of order) {
      const p = `task/${t.task.addr}`, open = opened.get(p) ?? t.running;
      // (The one running, marked; one not run yet by the cursor, dim.)
      all.push(el("tr", { className: `frame task${t.running ? " on" : ""}${t.last == null ? " later" : ""}`,
                          title: t.running ? "running at the cursor" : t.last == null ? "runs only after the cursor" : "" }, el("td", { colSpan: 3, style: `padding-left: ${8 + depth * 16}px` },
        el("a", { className: "tw", textContent: open ? "▾ " : "▸ ", title: open ? "fold it" : "its task_struct, as it was at the cursor", onclick: () => (opened.set(p, !open), redraw()) }),
        el("a", { className: "mono", textContent: taskName(t.task), title: t.running ? "" : "go to its last moment (its first, if it runs later)",
                  onclick: () => app.time.go(t.last ?? t.first) }),
        el("span", { className: "dim", textContent: t.last == null || !(t.syscall ?? t.func) ? "" : `  in ${t.syscall ?? t.func}` }))));
      if (!open) continue;
      rows(m.task_fields(t.task.addr).fields, p, 1, all);
    }
    body.replaceChildren(el("table", { className: "objs stack" }, el("tbody", {}, ...all)));
  }

  // The Tasks tab, task `addr` open (the Stack's current task: a link here).
  function open(addr) {
    opened.set(`task/${addr}`, true);
    app.tab("tasks");
    redraw();
  }

  return { draw: () => draw(), mark: (i) => draw(i ?? app.time.cursor()), open };
}
