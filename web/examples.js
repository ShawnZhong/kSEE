// The examples. An example is a question about the kernel, a small C
// program that asks it (user/examples/*.c, run as a real process), and
// stops with a note each. Opening an example runs its program, recorded,
// to its end (the page opens the first once the kernel is up); then its
// recording is in the timeline. Its stops are places in it: the first call
// of the stop's function in the syscalls of the lines marked "// stops
// here" (of any task, for a stop that says so). Next goes from one to the
// next. Click a line of the program for what the kernel did for it.
import { $, el } from "./dom.js";
import { loadMonaco, readOnly } from "./editor.js";

export async function examples(app) {
  const { m, proc } = app;
  const list = await fetch(new URL("./examples/index.json", location)).then((r) => (r.ok ? r.json() : [])).catch(() => []);
  // The open example, its run ({ pid }), and its stops' records.
  const t = { tour: null, first: 1, source: [], run: null, running: false, marks: null, ed: null, line: null, at: [], k: -1 };

  function index() {
    t.tour = null;
    const row = (x) => el("div", { className: "row example", onclick: () => open(x) },
      el("div", { className: "tt", textContent: x.title }), x.question ? el("div", { className: "meta", textContent: x.question }) : null);
    $("examples").replaceChildren(
      el("div", { className: "note", textContent: "Each asks a question about the kernel, and answers it with its recording: open one, then follow its stops." }),
      ...list.map(row),
      el("div", { className: "note dim", textContent: "Your own: a C file in user/examples/ (on nolibc), then ./ksee.sh build." }));
  }

  async function open(tour) {
    t.tour = tour;
    t.run = null;
    t.at = [];
    t.k = -1;
    const text = await fetch(new URL(`./examples/${tour.name}.c`, location)).then((r) => r.text());
    // The program, without the tour's header comments; its "// stops here"
    // marks shown as ● instead.
    const lines = text.split("\n");
    const skip = lines.findIndex((l) => !l.startsWith("// "));
    t.first = skip + 1;
    t.source = lines.slice(skip).map((l) => l.replace(/\s*\/\/ stops here\s*$/, ""));
    t.status = el("div", { className: "tstatus dim" });
    const box = el("div", { className: "prog", style: `height:${Math.min(t.source.length, 40) * 18 + 8}px` });
    $("examples").replaceChildren(head(tour, el("button", { textContent: "↻ Run again", onclick: () => run() })),
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

  // An example's heading (and `button`), and its stops.
  function head(tour, button) {
    t.stops = el("ol", { className: "stops" }, ...(tour.stops ?? []).map((s, k) =>
      el("li", { onclick: () => stop(k) }, el("b", { className: "mono", textContent: s.func }), " ", s.note)));
    return el("div", { className: "thead" },
      el("a", { textContent: "‹ examples", onclick: index }),
      el("div", { className: "tt", textContent: tour.title }),
      tour.question ? el("div", { className: "q", textContent: tour.question }) : null,
      button ? el("div", { className: "tbar" }, button) : null);
  }

  // The kernel is up: run the first example (unless one is open).
  function ready() {
    if (!t.tour && list.length) open(list[0]);
  }

  // Run the program, recorded, to its end; then its stops.
  async function run() {
    if (t.running) return;
    t.running = true;
    t.status.textContent = "running…";
    m.record(true);
    try {
      const { pid, status } = await proc.run(t.tour.path);
      if (status < 0 || !pid) throw new Error(`could not run ${t.tour.path}: ${status}`);
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
  if (app.ready) ready();
  // (For tests: the program line marked now.)
  return { follow, open, run, next, stop, list, pickLine, ready, marked: () => t.line };
}
