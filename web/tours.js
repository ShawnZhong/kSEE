// Tours and programs. A tour is a question about the kernel, a small C
// program that asks it (user/tours/*.c, run as a real process), and stops
// with a note each; a program (user/programs/*.c) is just the program.
// Opening one runs it, recorded, to its end; then its recording is in the
// timeline. A tour's stops are places in it: the first call of the stop's
// function in the syscalls of the lines marked "// stops here" (of any
// task, for a stop that says so). Next goes from one to the next. Click a
// line of the program for what the kernel did for it.
import { $, el } from "./dom.js";
import { loadMonaco, readOnly } from "./editor.js";

export async function tours(app) {
  const { m, proc } = app;
  const list = await fetch(new URL("./tours/index.json", location)).then((r) => (r.ok ? r.json() : [])).catch(() => []);
  // The open tour (or program), its run ({ pid }), and its stops' records.
  const t = { tour: null, first: 1, source: [], run: null, running: false, marks: null, ed: null, line: null, at: [], k: -1 };

  function index() {
    t.tour = null;
    const row = (x) => el("div", { className: "row tour", onclick: () => open(x) },
      el("div", { className: "tt", textContent: x.title }), x.question ? el("div", { className: "meta", textContent: x.question }) : null);
    $("tours").replaceChildren(
      el("div", { className: "note", textContent: "A tour asks a question, and answers it with a small C program: open one to run it, recorded, then follow its stops." }),
      ...list.filter((x) => !x.program).map(row),
      el("h3", { textContent: "Programs" }),
      el("div", { className: "note", textContent: "Your own: a C file in user/programs/ (on nolibc), then ./ksee.sh build." }),
      ...list.filter((x) => x.program).map(row));
  }

  async function open(tour) {
    t.tour = tour;
    t.run = null;
    t.at = [];
    t.k = -1;
    const text = await fetch(new URL(`./tours/${tour.name}.c`, location)).then((r) => r.text());
    // The program, without the tour's header comments; its "// stops here"
    // marks shown as ● instead.
    const lines = text.split("\n");
    const skip = tour.program ? 0 : lines.findIndex((l) => !l.startsWith("// "));
    t.first = skip + 1;
    t.source = lines.slice(skip).map((l) => l.replace(/\s*\/\/ stops here\s*$/, ""));
    t.stops = el("ol", { className: "stops" }, ...(tour.stops ?? []).map((s, k) =>
      el("li", { onclick: () => stop(k) }, el("b", { className: "mono", textContent: s.func }), " ", s.note)));
    t.status = el("div", { className: "tstatus dim" });
    const box = el("div", { className: "prog", style: `height:${Math.min(t.source.length, 40) * 18 + 8}px` });
    $("tours").replaceChildren(
      el("div", { className: "thead" },
        el("a", { textContent: "‹ tours and programs", onclick: index }),
        el("div", { className: "tt", textContent: tour.title }),
        tour.question ? el("div", { className: "q", textContent: tour.question }) : null,
        el("div", { className: "tbar" }, el("button", { textContent: "↻ Run again", onclick: () => run() }))),
      tour.stops?.length ? t.stops : null, t.status, box);
    const monaco = await loadMonaco();
    t.ed?.dispose();
    t.ed = monaco.editor.create(box, { ...readOnly, glyphMargin: true, minimap: { enabled: false }, stickyScroll: { enabled: false },
      lineNumbers: (n) => `${n + t.first - 1}`, lineNumbersMinChars: 3, folding: false,
      model: monaco.editor.createModel(t.source.join("\n"), "c", monaco.Uri.parse(`tour:///${tour.name}.c#${Date.now()}`)) });
    t.marks = t.ed.createDecorationsCollection();
    t.ed.onMouseDown((e) => e.target.position && pickLine(e.target.position.lineNumber + t.first - 1));
    t.line = null;
    decorate();
    run();
  }

  // Run the program, recorded, to its end; then its stops.
  async function run() {
    if (t.running) return;
    t.running = true;
    t.status.textContent = "running…";
    m.record(true);
    try {
      const pid = await proc.spawn(t.tour.path);
      if (pid < 0) throw new Error(`could not start ${t.tour.path}: ${pid}`);
      const status = await proc.wait(pid);
      t.run = { pid };
      t.status.textContent = `${t.tour.path} ran as pid ${pid} (exit status ${status}): click a line for what the kernel did for it.`;
    } catch (e) {
      t.status.textContent = e.message;
    } finally {
      m.record(false);
      t.running = false;
    }
    if (!t.run) return;
    app.refresh();
    app.selectProgram(t.run.pid, `${t.tour.name} (pid ${t.run.pid})`);
    t.at = stopsAt();
    if (t.at.some((i) => i != null)) stop(t.at.findIndex((i) => i != null));
  }

  // Each stop's record (null if it is not in the recording).
  function stopsAt() {
    const p = prog();
    if (!p || !t.tour.stops) return [];
    // The spans to look in: the syscalls of the marked lines (else all of the program).
    const spans = (t.tour.lines?.length ? p.lines.filter((l) => t.tour.lines.includes(l.line)) : [])
      .map((l) => app.syscallAfter(p.task, l.i)).filter(Boolean).map((s) => ({ a: s.first, b: s.last ?? m.records() - 1 }));
    if (!spans.length) spans.push({ a: p.first, b: p.last });
    return t.tour.stops.map((s) => {
      for (const { a, b } of spans) {
        const i = m.first_call(s.func, a, b, s.any ? undefined : p.task);
        if (i != null) return i;
      }
      return null;
    });
  }
  // Go to stop `k`: the cursor there, and its note.
  function stop(k) {
    if (t.at[k] == null) return;
    t.k = k;
    t.stops.querySelectorAll("li").forEach((li, j) => {
      li.classList.toggle("on", j === k);
      li.classList.toggle("done", j < k);
    });
    app.goStop(t.at[k], t.tour.stops[k], k, t.tour.stops.length);
  }
  const next = () => {
    for (let k = t.k + 1; k < t.at.length; k++) if (t.at[k] != null) return stop(k);
  };

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
        glyphMarginHoverMessage: { value: "the tour's stops are in this line's syscall" } } })),
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

  // The cursor moved (at: Machine.at's).
  function follow(at) {
    const l = t.tour && lineAt(at?.i);
    if (l) mark(l.line);
  }

  index();
  // (For tests: the program line marked now.)
  return { follow, open, run, next, stop, list, pickLine, marked: () => t.line };
}
