// The source view: a whole kernel file in CodeMirror (web/editor.js), with
// the lines that ran (green; how often: hover the line), the function's
// lines, and the cursor. Hover a name for its value at the cursor (an
// argument, a global, and fields: file->f_pos); F12 or Ctrl+click on a
// name goes to its definition (a function, inline ones too, a type, a
// global, a field: Machine.definition); over the shown function, who
// called it in the recording (Machine.callers), each a link to the first
// such call; F4 runs to the line (Shift+F4: back to its last run).
import { $, el } from "./dom.js";
import { CM, base } from "./editor.js";

const KEYWORDS = new Set(("auto break case char const continue default do double else enum extern float for goto if " +
  "inline int long register restrict return short signed sizeof static struct switch typedef union unsigned void " +
  "volatile while bool true false NULL").split(" "));
const WORD = /[A-Za-z0-9_]/;

// The expression at offset `k` of `text`: the name there, with the fields
// before it ("file->f_pos" on f_pos); null if not a name.
export function exprAt(text, k) {
  if (!WORD.test(text[k] ?? "")) return null;
  let a = k, b = k;
  while (a > 0 && WORD.test(text[a - 1])) a--;
  while (b < text.length && WORD.test(text[b])) b++;
  let expr = text.slice(a, b);
  if (/^\d/.test(expr) || KEYWORDS.has(expr)) return null;
  for (;;) {
    const m = text.slice(0, a).match(/([A-Za-z_]\w*)\s*(->|\.)\s*$/);
    if (!m) break;
    expr = `${m[1]}${m[2]}${expr}`;
    a -= m[0].length;
  }
  return { expr, start: a, end: b };
}

export function source(app) {
  const { m } = app;
  const texts = new Map();
  const fetchFile = (file) => {
    if (!texts.has(file))
      texts.set(file, file.startsWith("/") ? Promise.resolve(null)
        : fetch(new URL(`./src/${file}`, location)).then((r) => (r.ok ? r.text() : null)).catch(() => null));
    return texts.get(file);
  };
  // What is shown, and the cursor's line.
  const v = { file: null, fn: null, syscall: undefined, cursor: null, counts: new Map() };
  // A file's state (its document), kept once made.
  const states = new Map();

  // ---- The marks: a state field of line decorations, set as a whole ----
  const setMarks = CM.StateEffect.define();
  const marks = CM.StateField.define({
    create: () => CM.Decoration.none,
    update(set, tr) {
      for (const e of tr.effects) if (e.is(setMarks)) return e.value;
      return set.map(tr.changes);
    },
    provide: (f) => CM.EditorView.decorations.from(f),
  });
  // The cursor's ▶, in a gutter of its own.
  const cursorMark = new (class extends CM.GutterMarker { toDOM() { return el("span", { className: "gl-cursor" }); } })();
  const cursorGutter = CM.gutter({
    class: "cm-cursorGutter",
    lineMarker: (view, line) => (v.cursor?.file === v.file && view.state.doc.lineAt(line.from).number === v.cursor.line ? cursorMark : null),
    lineMarkerChange: (update) => update.transactions.some((tr) => tr.effects.some((e) => e.is(setMarks))),
  });
  // Who called the shown function: a block above its first line.
  class Lens extends CM.WidgetType {
    constructor(dom) { super(); this.dom = dom; }
    toDOM() { return this.dom; }
    eq(other) { return other.dom === this.dom; }
  }
  function lens(fn) {
    const cs = m.callers(fn.name, v.syscall), n = cs.reduce((a, c) => a + c.count, 0);
    const where = v.syscall != null ? "in this syscall" : "in the recording";
    const item = (text, i) => (i == null ? el("span", { textContent: text })
      : el("a", { textContent: text, title: "go to this call", onclick: () => app.time.go(i) }));
    return el("div", { className: "lens dim" }, item(n ? `called ${n}× ${where} from` : `not called ${where}`),
      ...cs.slice(0, 6).map((c) => item(`${c.func}${c.line ? `:${c.line}` : ""}${c.count > 1 ? ` (${c.count}×)` : ""}`, c.first)),
      ...(cs.length > 6 ? [item(`and ${cs.length - 6} more`)] : []));
  }

  // ---- Hover: the value at the cursor ----
  const tips = CM.hoverTooltip((view, pos) => {
    const line = view.state.doc.lineAt(pos);
    const h = hover(line.number, line.text, pos - line.from);
    return h && { pos: line.from + h.start, end: line.from + h.end, above: true,
                  create: () => ({ dom: el("pre", { className: "hovertip mono", textContent: h.text }) }) };
  }, { hoverTime: 200 });

  // ---- Keys and clicks: definitions, run to a line ----
  const lineOf = (view) => view.state.doc.lineAt(view.state.selection.main.head).number;
  const keys = CM.keymap.of([
    { key: "F12", run: (view) => (definitionAt(view.state.selection.main.head), true) },
    { key: "F4", run: (view) => (app.runTo(v.file, lineOf(view), 1), true) },
    { key: "Shift-F4", run: (view) => (app.runTo(v.file, lineOf(view), -1), true) },
  ]);
  const clicks = CM.EditorView.domEventHandlers({
    mousedown(e, view) {
      if (!(e.ctrlKey || e.metaKey)) return false;
      const pos = view.posAtCoords({ x: e.clientX, y: e.clientY });
      if (pos != null) definitionAt(pos);
      return true;
    },
  });

  const ed = new CM.EditorView({ parent: $("src"), state: CM.EditorState.create({ doc: "", extensions: base }) });
  $("src").querySelector(".empty")?.remove();

  function state(file, text) {
    if (!states.has(file)) states.set(file, CM.EditorState.create({ doc: text, extensions: [base, marks, cursorGutter, tips, keys, clicks] }));
    return states.get(file);
  }

  // Whether line `n` of the shown file is in the shown function.
  function inFunction(n) {
    const fn = v.fn?.file === v.file ? v.fn : null;
    return fn && n >= fn.line && n <= fn.last;
  }

  // The value of the expression at `k` in line `line` (its `text`), for a
  // hover: { start, end, text }, or null.
  function hover(line, text, k) {
    const e = exprAt(text, k);
    // (Arguments only in the shown function's own lines; elsewhere, globals.)
    const o = e && app.stack.resolve(e.expr, inFunction(line) ? v.fn.name : null);
    if (!o) return null;
    const fields = o.value == null || (o.target != null && o.value !== "NULL") ? (app.stack.kids(o) ?? []).slice(0, 10) : [];
    const code = [`${e.expr} = ${o.value ?? "{…}"}  // ${o.ty}`, ...fields.map((c) => `  .${c.name} = ${c.value ?? "{…}"}`)].join("\n");
    return { start: e.start, end: e.end, text: code };
  }
  // The definition of `word` (`expr`: the expression it ends, for a field:
  // file->f_pos): { file, line, func? (an instrumented function: its
  // source, with the lines that ran) }, or null.
  function definition(word, expr) {
    const parent = expr?.match(/^(.*\S)\s*(?:->|\.)\s*\w+$/)?.[1];
    // (A field: of the struct the expression before it is, by name.)
    const owner = parent && app.stack.resolve(parent)?.ty.match(/^(?:const )?(?:struct|union) (\w+)/)?.[1];
    // (A field of a value the stack does not have: of any struct with it.)
    const d = m.definition(owner ? `${owner}.${word}` : parent ? `.${word}` : word);
    return d && { file: d.file, line: d.line, func: !owner && m.functions(word).length ? word : null };
  }
  // Go to the definition of the name at `pos` of the shown file (a
  // function: its source, with the lines that ran; else the file, at the
  // line). Whether there was one.
  function definitionAt(pos) {
    const doc = ed.state.doc, line = doc.lineAt(pos), w = ed.state.wordAt(pos);
    if (!w) return false;
    const word = doc.sliceString(w.from, w.to), jump = definition(word, exprAt(line.text, pos - line.from)?.expr);
    if (!jump) return false;
    if (jump.func) showFunction(jump.func);
    else {
      saved = null;
      v.fn = null;
      showFile(jump.file, jump.line);
    }
    return true;
  }

  // The marks for the shown file: the lines that ran, the function, the
  // cursor, the lens.
  function decorate() {
    const file = v.file, fn = v.fn?.file === file ? v.fn : null, doc = ed.state.doc;
    const at = (n) => n >= 1 && n <= doc.lines && doc.line(n).from;
    const where = v.syscall != null ? " in this syscall" : " in the recording";
    const d = [];
    for (const [n, c] of v.counts) {
      if (at(n) !== false) d.push(CM.Decoration.line({ class: "ln-hit", attributes: { title: `ran ${c}×${where}` } }).range(at(n)));
    }
    if (fn) {
      for (let n = fn.line; n <= Math.min(fn.last, doc.lines); n++) d.push(CM.Decoration.line({ class: "ln-infn" }).range(at(n)));
      if (at(fn.line) !== false) d.push(CM.Decoration.widget({ widget: new Lens(lens(fn)), block: true, side: -1 }).range(at(fn.line)));
    }
    if (v.cursor?.file === file && at(v.cursor.line) !== false) d.push(CM.Decoration.line({ class: "ln-cursor" }).range(at(v.cursor.line)));
    d.sort((x, y) => x.from - y.from || x.value.startSide - y.value.startSide);
    ed.dispatch({ effects: setMarks.of(CM.Decoration.set(d, true)) });
  }
  // Scroll line `n` into view: to the top, or (`center`) the middle if it
  // is not in view.
  function reveal(n, center) {
    const doc = ed.state.doc;
    if (n < 1 || n > doc.lines) return;
    const pos = doc.line(n).from;
    ed.dispatch({ selection: { anchor: pos }, effects: CM.EditorView.scrollIntoView(pos, { y: center ? "center" : "start", yMargin: center ? 0 : 4 }) });
  }

  async function showFile(file, focus) {
    v.file = file;
    v.focus = focus;
    const text = await fetchFile(file);
    if (v.file !== file) return; // changed while loading
    const st = state(file, text ?? `// ${file}: not in this site's sources.`);
    if (ed.state !== st) ed.setState(st);
    v.counts = new Map(text === null ? [] : m.file_coverage(file, v.syscall).map((c) => [c.line, c.count]));
    decorate();
    const fn = v.fn?.file === file ? v.fn : null;
    reveal(focus ?? fn?.line ?? 1, !!focus);
    const ran = !fn ? "" : ` · ${fn.name}` + (fn.sealed ? ": sealed (it loops, only reading memory: its call and return are recorded, not its lines)" : "");
    $("fn-title").textContent = (saved ? "preview · " : "") + file + ran;
    $("fn-title").classList.toggle("preview", !!saved);
  }

  // Function `name` (the lines that ran: in syscall `syscall`), in `file`
  // (a block's own, when it is inlined from a header) at line `focus`.
  function show(name, syscall, focus, file) {
    const [f] = m.functions(name);
    if (!f) return;
    v.fn = f;
    v.syscall = syscall;
    showFile(file ?? f.file, focus);
  }
  function showFunction(name, syscall, focus, file) {
    saved = null;
    show(name, syscall, focus, file);
  }

  // A preview (hovering a call): function `func` at `file:line`, until
  // unpreview goes back to what was shown (or another showFunction keeps it).
  let saved = null;
  function preview(func, file, line) {
    saved ??= { fn: v.fn, syscall: v.syscall, cursor: v.cursor, file: v.file, focus: v.focus, top: ed.scrollDOM.scrollTop };
    v.cursor = { file, line };
    show(func, v.syscall, line, file);
  }
  async function unpreview() {
    if (!saved) return;
    const was = saved;
    saved = null;
    Object.assign(v, { fn: was.fn, syscall: was.syscall, cursor: was.cursor });
    if (was.file) await showFile(was.file, was.focus);
    if (!saved && was.top != null) ed.scrollDOM.scrollTop = was.top;
  }

  // The shown file's lines (for tests), and the first line at or after
  // `from` with `needle` in it: { line, column (of needle) }, or null.
  const text = () => ed.state.doc.toString();
  function find(needle, from = 1) {
    const doc = ed.state.doc;
    for (let n = from; n <= doc.lines; n++) {
      const k = doc.line(n).text.indexOf(needle);
      if (k >= 0) return { line: n, column: k + 1 };
    }
    return null;
  }
  // For tests: the hover at the first `needle` of the shown function (and
  // after it), on `word`.
  function hoverAt(needle, word) {
    const at = find(needle, v.fn?.file === v.file ? v.fn.line : 1);
    return at ? hover(at.line, ed.state.doc.line(at.line).text, at.column - 1 + needle.indexOf(word))?.text ?? null : null;
  }
  // For tests: go to the definition of the name at `line`, `column`.
  const definitionAtLine = (line, column) => definitionAt(ed.state.doc.line(line).from + column - 1);

  return { view: v, showFunction, preview, unpreview, hoverAt, text, find, definitionAt: definitionAtLine,
           lineAt: () => ed.state.doc.lineAt(ed.state.selection.main.head).number };
}
