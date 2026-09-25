// Tours: a question about the kernel, a small C program that asks it
// (user/tours/*.c, run as a real process), and stops in the kernel with a
// note each. Opening a tour runs it: the program, recorded, with a
// breakpoint at each stop; at a stop, the note says what is happening
// (Continue goes on, Play goes line by line, Finish skips the other stops).
// Afterwards the program's run is in the history: its lines link to what
// the kernel did for them.
import { $, el } from "./dom.js";
import { loadMonaco, readOnly } from "./editor.js";

export async function tours(app) {
  const { m, proc } = app;
  const list = await fetch(new URL("./tours/index.json", location)).then((r) => (r.ok ? r.json() : [])).catch(() => []);
  // The open tour: { tour, lines (the program's source), run: { pid, prog } }.
  const t = { tour: null, source: [], rows: [], run: null, running: false, reached: new Set() };

  function index() {
    t.tour = null;
    app.tourStops = null;
    $("tours").replaceChildren(el("div", { className: "note", textContent: "Each tour asks a question, and answers it with a small C program run in a real process: follow what the kernel does for it, stop by stop." }),
      ...list.map((tour) => el("div", { className: "row tour", onclick: () => open(tour).then(() => run()) },
        el("div", { className: "tt", textContent: tour.title }), el("div", { className: "meta", textContent: tour.question }))));
  }

  async function open(tour) {
    t.tour = tour;
    t.run = null;
    t.reached = new Set();
    const text = await fetch(new URL(`./tours/${tour.name}.c`, location)).then((r) => r.text());
    // The program without the tour's own header comments.
    const lines = text.split("\n");
    const skip = lines.findIndex((l) => !l.startsWith("// "));
    t.first = skip + 1;
    // (A line's "// stops here" marks where the tour stops: a ● instead.)
    t.source = lines.slice(skip).map((l) => l.replace(/\s*\/\/ stops here\s*$/, ""));
    const box = el("div", { className: "prog", style: `height:${Math.min(t.source.length, 40) * 18 + 8}px` });
    t.stops = el("ol", { className: "stops" }, ...tour.stops.map((s) =>
      el("li", { dataset: { func: s.func } }, el("b", { className: "mono", textContent: s.func }), " ", s.note)));
    t.status = el("div", { className: "tstatus dim" });
    $("tours").replaceChildren(
      el("div", { className: "thead" },
        el("a", { textContent: "‹ tours", onclick: index }),
        el("div", { className: "tt", textContent: tour.title }),
        el("div", { className: "q", textContent: tour.question }),
        el("div", { className: "tbar" },
          el("button", { textContent: "↻ Run again", onclick: () => run() }))),
      t.stops, t.status, box);
    app.tourStops = tour.stops;
    // The program, in Monaco (its lines as in the file: numbered from t.first).
    const monaco = await loadMonaco();
    t.ed?.dispose();
    t.ed = monaco.editor.create(box, { ...readOnly, glyphMargin: true, minimap: { enabled: false }, stickyScroll: { enabled: false },
      lineNumbers: (n) => `${n + t.first - 1}`, lineNumbersMinChars: 3, folding: false,
      model: monaco.editor.createModel(t.source.join("\n"), "c", monaco.Uri.parse(`tour:///${tour.name}.c#${Date.now()}`)) });
    t.marks = t.ed.createDecorationsCollection();
    t.ed.onMouseDown((e) => e.target.position && pickLine(e.target.position.lineNumber + t.first - 1));
    t.line = null;
    decorate();
  }

  // Run the program, recording, with its stops (breakpoints).
  async function run() {
    if (t.running) return;
    t.running = true;
    t.reached = new Set();
    t.stops.querySelectorAll("li").forEach((li) => li.classList.remove("on", "done"));
    const funcs = (t.funcs = t.tour.stops.map((s) => s.func));
    for (const f of funcs) m.break_function(f, true);
    app.tour = { pid: null, stops: t.tour.stops, lines: t.tour.lines };
    m.record(true);
    t.status.textContent = "starting the program…";
    try {
      const pid = await proc.spawn(t.tour.path);
      if (pid < 0) throw new Error(`could not start ${t.tour.path}: ${pid}`);
      app.tour.pid = pid;
      t.status.textContent = `${t.tour.path} runs as pid ${pid}; it stops at each stop`;
      const status = await proc.wait(pid);
      t.status.textContent = `${t.tour.path} (pid ${pid}) exited with status ${status}: click a line to see what the kernel did for it, or replay it (▶ Play).`;
      t.run = { pid };
    } catch (e) {
      t.status.textContent = e.message;
    } finally {
      for (const f of funcs) m.break_function(f, false);
      m.record(false);
      app.tour = null;
      t.running = false;
    }
    if (t.run) {
      app.refresh();
      app.selectProgram(t.run.pid, `${t.tour.name} (pid ${t.run.pid})`);
    }
  }

  // The program's line at record `i` (its latest Line record at or before).
  // (Asked again each time: records move as the ring goes on.)
  const prog = () => (t.run ? m.program(t.run.pid) : null);
  function lineAt(i) {
    const lines = prog()?.lines;
    if (!lines?.length || i == null) return null;
    let best = null;
    for (const l of lines) if (l.i <= i) best = l;
    return best;
  }
  // The program's markers: the lines the stops are for, the line now.
  function decorate() {
    const Range = globalThis.monaco?.Range;
    if (!t.ed || !Range) return;
    const at = (line) => new Range(line - t.first + 1, 1, line - t.first + 1, 1);
    t.marks.set([
      ...(t.tour.lines ?? []).map((l) => ({ range: at(l), options: { glyphMarginClassName: "gl-stophere",
        glyphMarginHoverMessage: { value: "the tour stops in this line's syscall" } } })),
      ...(t.line ? [{ range: at(t.line), options: { isWholeLine: true, className: "ln-cursor", glyphMarginClassName: "gl-cursor" } }] : []),
    ]);
  }
  function mark(line) {
    t.line = line;
    decorate();
    t.ed?.revealLineInCenterIfOutsideViewport(line - t.first + 1);
  }
  // A program line: to what the kernel did for it (its next syscall).
  function pickLine(line) {
    const p = prog();
    const l = p?.lines.findLast((l) => l.line === line);
    if (!l) return;
    const s = app.syscallAfter(p.task, l.i);
    if (s) app.select(s);
  }

  // The cursor moved (at: Machine.at's, or null: now); stopped: a stop.
  function follow(at, stopped) {
    if (!t.tour) return;
    if (stopped && app.tour) {
      // While running: the stop's note; the line from the live trace.
      const k = t.tour.stops.findIndex((s) => s.func === stopped.func);
      t.stops.querySelectorAll("li").forEach((li, j) => {
        li.classList.toggle("on", j === k);
        if (j === k) t.reached.add(j);
        li.classList.toggle("done", t.reached.has(j) && j !== k);
      });
      const prog = m.program(app.tour.pid ?? stopped.task.pid);
      if (prog?.lines.length) mark(prog.lines.at(-1).line);
      return;
    }
    const l = lineAt(at?.i);
    if (l) mark(l.line);
  }

  // No more stops: run the program to its end.
  function finish() {
    for (const f of t.funcs ?? []) m.break_function(f, false);
    if (app.tour) app.tour.stops = [];
    m.resume();
  }

  index();
  // (For tests: the program line marked now.)
  return { follow, open, run, finish, list, pickLine, marked: () => t.line };
}
