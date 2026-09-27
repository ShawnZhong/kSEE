// The source view: a whole kernel file in CodeMirror (web/editor.js), with
// the lines that ran (green; how often: hover the line), the function's
// lines, and the cursor. Hover a name for its value at the cursor (an
// argument, a global, and fields: file->f_pos), or, for a type, a field
// or a global with no value here, its declaration's first lines; F12 or
// Ctrl+click on a
// name goes to its definition (a function, inline ones too, a type, a
// global, a field: Machine.definition); F4 runs to the line (Shift+F4:
// back to its last run).
import { $, el } from "./dom.js";
import { CM, base } from "./editor.js";

const KEYWORDS = new Set(("auto break case char const continue default do double else enum extern float for goto if " +
  "inline int long register restrict return short signed sizeof static struct switch typedef union unsigned void " +
  "volatile while bool true false NULL").split(" "));
const WORD = /[A-Za-z0-9_]/;

// The expression at offset `k` of `text`: the name there, with the fields
// before it ("file->f_pos" on f_pos); null if not a name.
function exprAt(text, k) {
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
  // ---- Hover: the value at the cursor; else a declaration, its first lines ----
  const tips = CM.hoverTooltip(async (view, pos) => {
    const line = view.state.doc.lineAt(pos);
    const h = hover(line.number, line.text, pos - line.from) ?? await declaration(line.text, pos - line.from);
    return h && { pos: line.from + h.start, end: line.from + h.end, above: true,
                  create: () => ({ dom: el("pre", { className: "hovertip mono", textContent: h.text }) }) };
  }, { hoverTime: 200 });
  // The declaration of the name at `k` of `text` (a type, a field, a global,
  // an inline function: not a value the recording has): where, and its
  // first lines, for a hover; null if none.
  async function declaration(text, k) {
    const e = exprAt(text, k), w = e && text.slice(e.start, e.end);
    const d = w && !m.functions(w).length && definition(w, e.expr);
    if (!d) return null;
    const src = await fetchFile(d.file);
    if (!src) return null;
    const lines = src.split("\n").slice(d.line - 1, d.line + 7);
    return { start: e.start, end: e.end, text: [`${d.file}:${d.line}`, ...lines].join("\n") };
  }

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
      v.fn = null;
      showFile(jump.file, jump.line);
    }
    return true;
  }

  // The marks for the shown file: the lines that ran, the function, the
  // cursor.
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
    }
    if (v.cursor?.file === file && at(v.cursor.line) !== false) d.push(CM.Decoration.line({ class: "ln-cursor" }).range(at(v.cursor.line)));
    d.sort((x, y) => x.from - y.from || x.value.startSide - y.value.startSide);
    ed.dispatch({ effects: setMarks.of(CM.Decoration.set(d, true)) });
  }
  // Show line `n`: as a debugger does, a jump (`jump`: another file or
  // function, a link) puts it a third of the way down, with the lines
  // before it for context; stepping leaves the view alone while the line
  // stays in the middle band of it, and brings it back to that third when
  // it leaves.
  function reveal(n, jump) {
    const doc = ed.state.doc;
    if (n < 1 || n > doc.lines) return;
    const pos = doc.line(n).from;
    ed.dispatch({ selection: { anchor: pos } });
    const box = ed.scrollDOM, top = ed.lineBlockAt(pos).top - box.scrollTop, h = box.clientHeight;
    if (!jump && top > h * 0.1 && top < h * 0.8) return;
    ed.dispatch({ effects: CM.EditorView.scrollIntoView(pos, { y: "start", yMargin: Math.max(0, h / 3) }) });
  }

  let was = { file: null, fn: null }; // (what the view showed last: a jump, or a step)
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
    // (A jump: another file, or another function; else a step in the same one.)
    const jump = file !== was.file || fn?.name !== was.fn;
    was = { file, fn: fn?.name };
    reveal(focus ?? fn?.line ?? 1, jump);
    const ran = !fn ? "" : ` · ${fn.name}` + (fn.sealed ? ": sealed (it loops, only reading memory: its call and return are recorded, not its lines)" : "");
    $("fn-title").textContent = file + ran;
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
  const showFunction = show;

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
  async function hoverAt(needle, word) {
    const at = find(needle, v.fn?.file === v.file ? v.fn.line : 1);
    if (!at) return null;
    const text = ed.state.doc.line(at.line).text, k = at.column - 1 + needle.indexOf(word);
    return (hover(at.line, text, k) ?? await declaration(text, k))?.text ?? null;
  }
  // For tests: go to the definition of the name at `line`, `column`.
  const definitionAtLine = (line, column) => definitionAt(ed.state.doc.line(line).from + column - 1);

  return { view: v, showFunction, hoverAt, text, find, definitionAt: definitionAtLine,
           lineAt: () => ed.state.doc.lineAt(ed.state.selection.main.head).number };
}
